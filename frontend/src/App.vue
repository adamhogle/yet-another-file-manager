<script setup>
import { computed, onMounted, ref } from 'vue';
import { buildDownloadHref, fetchDirectory, toChildPath } from './lib/api/client';
import { formatSize, formatModified } from './lib/format.js';

const title = 'Yet Another File Manager';
const repositoryUrl = 'https://github.com/adamhogle/yet-another-file-manager';
const currentYear = new Date().getFullYear();
const currentPath = ref('');
const entries = ref([]);
const errorMessage = ref('');
const isLoading = ref(true);

const breadcrumbs = computed(() => {
  const segments = currentPath.value ? currentPath.value.split('/').filter(Boolean) : [];
  const items = [{ label: 'root', path: '' }];

  for (let index = 0; index < segments.length; index += 1) {
    items.push({
      label: segments[index],
      path: segments.slice(0, index + 1).join('/')
    });
  }

  return items;
});

function readPathFromUrl() {
  return new URLSearchParams(window.location.search).get('p') ?? '';
}

function updateUrl(path) {
  const url = new URL(window.location.href);
  if (path) {
    url.searchParams.set('p', path);
  } else {
    url.searchParams.delete('p');
  }
  window.history.pushState({}, '', url);
}

async function loadDirectory(path, updateHistory = true) {
  isLoading.value = true;
  errorMessage.value = '';

  try {
    const listing = await fetchDirectory(path);
    currentPath.value = listing.currentPath;
    entries.value = listing.entries;

    if (updateHistory) {
      updateUrl(listing.currentPath);
    }
  } catch (error) {
    entries.value = [];
    errorMessage.value =
      error instanceof Error ? error.message : 'The shared directory is unavailable.';
  } finally {
    isLoading.value = false;
  }
}

function openDirectory(path) {
  loadDirectory(path, true);
}

onMounted(() => {
  loadDirectory(readPathFromUrl(), false);
  window.addEventListener('popstate', () => {
    loadDirectory(readPathFromUrl(), false);
  });
});
</script>

<template>
  <div class="page-shell">
    <header class="page-header">
      <h1>
        <img class="app-logo" src="/yafm-logo.svg" alt="Yet Another File Manager logo" />
        <span>{{ title }}</span>
      </h1>
    </header>

    <section v-if="errorMessage" class="panel error-panel">
      <h2>Directory unavailable</h2>
      <p>{{ errorMessage }}</p>
    </section>

    <template v-else>
      <section class="panel breadcrumb-panel">
        <nav class="breadcrumb-row" aria-label="Current directory">
          <template v-for="(crumb, index) in breadcrumbs" :key="crumb.path || 'root'">
            <button
              type="button"
              class="breadcrumb-link"
              :class="{ 'is-current': index === breadcrumbs.length - 1 }"
              :aria-current="index === breadcrumbs.length - 1 ? 'page' : undefined"
              @click="openDirectory(crumb.path)"
            >
              {{ crumb.label }}
            </button>
            <span v-if="index < breadcrumbs.length - 1" class="breadcrumb-separator">/</span>
          </template>
        </nav>
      </section>

      <section class="panel">
        <p v-if="isLoading" class="empty-state">Loading directory...</p>
        <p v-else-if="entries.length === 0" class="empty-state">This directory is empty.</p>

        <div v-else class="details-grid">
          <div class="details-header details-row">
            <span>Name</span>
            <span>Size</span>
            <span>Modified</span>
            <span class="actions-header">Actions</span>
          </div>

          <div v-for="entry in entries" :key="`${entry.kind}:${entry.name}`" class="details-row">
            <div class="name-cell">
              <button
                v-if="entry.kind === 'directory'"
                type="button"
                class="name-link"
                @click="openDirectory(toChildPath(currentPath, entry.name))"
              >
                {{ entry.name }}
              </button>
              <span v-else class="entry-name">{{ entry.name }}</span>
            </div>
            <span class="size-cell">{{ formatSize(entry.sizeBytes) }}</span>
            <span class="date-cell">{{ formatModified(entry.modifiedAt) }}</span>
            <span class="actions-cell">
              <a
                v-if="entry.kind === 'file'"
                class="icon-action"
                :href="buildDownloadHref(currentPath, entry.name)"
                :aria-label="`Download ${entry.name}`"
                title="Download"
              >
                <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                  <path
                    d="M12 3c.55 0 1 .45 1 1v8.59l2.3-2.3a1 1 0 0 1 1.4 1.42l-4 3.97a1 1 0 0 1-1.4 0l-4-3.97a1 1 0 0 1 1.4-1.42l2.3 2.3V4c0-.55.45-1 1-1Zm-6 15h12a1 1 0 1 1 0 2H6a1 1 0 1 1 0-2Z"
                  />
                </svg>
              </a>
            </span>
          </div>
        </div>
      </section>
    </template>

    <footer class="page-footer" aria-label="Project license and source">
      <small>
        © {{ currentYear }} Yet Another File Manager contributors • AGPL-3.0-only •
        <a :href="repositoryUrl" target="_blank" rel="noopener noreferrer">GitHub</a>
      </small>
    </footer>
  </div>
</template>

<style scoped>
:global(body) {
  margin: 0;
  font-family: 'Segoe UI', 'IBM Plex Sans', sans-serif;
  background: #f4f6f9;
  color: #1f2833;
}

.page-shell {
  max-width: 90rem;
  margin: 0 auto;
  padding: 0.9rem 1rem 2rem;
}

.page-header {
  margin-bottom: 0.6rem;
}

.page-footer {
  margin-top: 0.8rem;
  text-align: center;
  color: #5b6d7f;
}

.page-footer small {
  font-size: 0.76rem;
}

.page-footer a {
  color: #0b57a1;
}

.page-footer a:hover {
  text-decoration: underline;
}

h1 {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  margin: 0;
  font-size: 1.05rem;
  font-weight: 600;
  line-height: 1.2;
}

.app-logo {
  width: 1.1rem;
  height: 1.1rem;
  border-radius: 0.2rem;
  display: block;
}

.panel {
  background: #ffffff;
  border: 1px solid #d8dee6;
  border-radius: 0.45rem;
}

.panel + .panel {
  margin-top: 0.5rem;
}

.breadcrumb-panel {
  padding: 0.45rem 0.65rem;
}

.breadcrumb-row {
  display: flex;
  align-items: center;
  gap: 0.2rem;
  flex-wrap: wrap;
  min-width: 0;
}

.breadcrumb-link {
  border: none;
  background: none;
  padding: 0;
  color: #0b57a1;
  font-size: 0.83rem;
  font-family: 'Cascadia Mono', 'Consolas', monospace;
  line-height: 1.35;
  text-decoration: none;
  cursor: pointer;
}

.breadcrumb-link:hover {
  text-decoration: underline;
}

.breadcrumb-link.is-current {
  color: #243242;
  font-weight: 600;
}

.breadcrumb-separator {
  color: #6a7a8a;
  font-size: 0.83rem;
  font-family: 'Cascadia Mono', 'Consolas', monospace;
}

.details-grid {
  padding: 0.1rem 0;
  display: grid;
  grid-template-columns: 1fr;
}

.details-row {
  display: grid;
  grid-template-columns: minmax(16rem, 1fr) 9rem 11.5rem 3.2rem;
  column-gap: 0.8rem;
  align-items: center;
  min-height: 1.85rem;
  padding: 0 0.6rem;
  border-bottom: 1px solid #edf1f5;
}

.details-header {
  position: sticky;
  top: 0;
  background: #f8fafc;
  font-size: 0.72rem;
  font-weight: 600;
  color: #516172;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  z-index: 1;
}

.name-cell,
.size-cell,
.date-cell,
.actions-cell {
  font-size: 0.82rem;
}

.actions-header,
.actions-cell {
  text-align: right;
}

.size-cell,
.date-cell {
  color: #46586a;
  font-variant-numeric: tabular-nums;
}

.entry-name,
.name-link {
  font-size: 0.82rem;
  line-height: 1.2;
}

.name-link {
  border: none;
  background: none;
  padding: 0;
  color: #0b57a1;
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-decoration: none;
  cursor: pointer;
}

.icon-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.5rem;
  height: 1.5rem;
  color: #0b57a1;
  border: 1px solid #d8e2ee;
  border-radius: 0.3rem;
  text-decoration: none;
  background: #f7fafc;
}

.icon-action svg {
  width: 0.92rem;
  height: 0.92rem;
  fill: currentColor;
}

.empty-state,
.error-panel p {
  padding: 0.8rem;
  margin: 0;
  font-size: 0.88rem;
}

.error-panel h2 {
  font-size: 0.95rem;
  margin: 0.8rem 0 0;
}

@media (max-width: 640px) {
  .page-shell {
    padding-inline: 0.55rem;
  }

  .breadcrumb-link,
  .breadcrumb-separator {
    font-size: 0.78rem;
  }

  .details-row {
    grid-template-columns: minmax(10rem, 1fr) 5.5rem 7.7rem 2.6rem;
    font-size: 0.75rem;
  }
}
</style>
