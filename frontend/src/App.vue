<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import {
  ApiHttpError,
  buildDownloadHref,
  deleteFile,
  fetchDirectory,
  fetchIdentity,
  toChildPath
} from './lib/api/client';
import type { DirectoryEntry, UserInfoResponse } from './lib/api/client';
import { formatSize, formatModified } from './lib/format';

const title = 'Yet Another File Manager';
const repositoryUrl = 'https://github.com/adamhogle/yet-another-file-manager';
const currentYear = new Date().getFullYear();
const currentPath = ref('');
const entries = ref<DirectoryEntry[]>([]);
const errorMessage = ref('');
const isLoading = ref(true);
const noAccess = ref(false);
const identity = ref<UserInfoResponse | null>(null);

// The logout endpoint: a plain browser navigation the gate exempts; the
// backend clears the session cookie and redirects to authentik's end-session
// endpoint.
const logoutHref = '/api/v1/auth/logout';

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

function readPathFromUrl(): string {
  return new URLSearchParams(window.location.search).get('p') ?? '';
}

function updateUrl(path: string): void {
  const url = new URL(window.location.href);
  if (path) {
    url.searchParams.set('p', path);
  } else {
    url.searchParams.delete('p');
  }
  window.history.pushState({}, '', url);
}

async function loadDirectory(path: string, updateHistory = true): Promise<void> {
  isLoading.value = true;
  errorMessage.value = '';
  noAccess.value = false;

  try {
    const listing = await fetchDirectory(path);
    // A navigation replaces the entries, so any confirmation armed on a
    // same-named row in the previous directory must not survive it.
    confirmingDelete.value = '';
    currentPath.value = listing.currentPath;
    entries.value = listing.entries;

    if (updateHistory) {
      updateUrl(listing.currentPath);
    }
  } catch (error) {
    entries.value = [];
    // A 404 on the root listing means the visible root is empty for this
    // account (the root always exists server-side, so 404 there can only be
    // no access): the explicit no-access state. Any other 404 is a
    // nonexistent path; other errors are shown as before. The status rides
    // the generated client's ApiHttpError.
    const status = error instanceof ApiHttpError ? error.status : undefined;
    if (status === 404 && path === '') {
      noAccess.value = true;
    } else {
      errorMessage.value =
        error instanceof Error ? error.message : 'The shared directory is unavailable.';
    }
  } finally {
    isLoading.value = false;
  }
}

function openDirectory(path: string): void {
  loadDirectory(path, true);
}

// The entry currently showing its inline delete confirmation. Only one row
// confirms at a time; clicking another row's delete action moves the
// confirmation there. Deletion is permanent, so the request is sent only
// after the second click.
const confirmingDelete = ref('');

function startDeleteConfirmation(entryName: string): void {
  confirmingDelete.value = entryName;
}

function cancelDeleteConfirmation(): void {
  confirmingDelete.value = '';
}

async function confirmDelete(entry: DirectoryEntry): Promise<void> {
  confirmingDelete.value = '';
  try {
    await deleteFile(currentPath.value, entry.name);
  } catch (error) {
    const status = error instanceof ApiHttpError ? error.status : undefined;
    if (status === 404) {
      // The file is already gone (a concurrent delete): reload the listing
      // so it reflects the filesystem, without surfacing an error.
      await loadDirectory(currentPath.value, false);
      return;
    }
    errorMessage.value =
      error instanceof Error ? error.message : 'The shared directory is unavailable.';
    return;
  }
  // Deleted: reload so the listing reflects the filesystem. updateHistory is
  // false because the path is unchanged.
  await loadDirectory(currentPath.value, false);
}

// Recovery from the error state: go back to the root, which replaces the
// stale path in the URL so a reload does not land in the error again.
function goToRoot(): void {
  loadDirectory('', true);
}

onMounted(() => {
  loadDirectory(readPathFromUrl(), false);
  window.addEventListener('popstate', () => {
    loadDirectory(readPathFromUrl(), false);
  });
  // The identity loads in the background: the directory listing must not
  // wait for it, and the menu renders when it arrives.
  fetchIdentity()
    .then((info) => {
      identity.value = info;
    })
    .catch(() => {
      identity.value = null;
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
      <nav v-if="identity" class="user-menu" aria-label="Account">
        <span class="user-name" :title="identity.email ?? identity.subject">{{
          identity.displayName
        }}</span>
        <a class="user-logout" :href="logoutHref" aria-label="Log out">Log out</a>
      </nav>
    </header>

    <section v-if="noAccess" class="panel notice-panel">
      <h2>No visible files</h2>
      <p>
        No folders are shared with your account. Ask the operator to add your groups to the access
        configuration.
      </p>
    </section>

    <section v-else-if="errorMessage" class="panel error-panel" role="alert">
      <div class="error-heading">
        <svg class="error-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path
            d="M12 2a10 10 0 1 1 0 20 10 10 0 0 1 0-20Zm0 2a8 8 0 1 0 0 16 8 8 0 0 0 0-16Zm0 10.9c.55 0 1 .45 1 1s-.45 1-1 1-1-.45-1-1 .45-1 1-1Zm0-8.9c.55 0 1 .45 1 1v5c0 .55-.45 1-1 1s-1-.45-1-1V7c0-.55.45-1 1-1Z"
          />
        </svg>
        <h2>Directory unavailable</h2>
      </div>
      <p>{{ errorMessage }}</p>
      <p class="error-actions">
        <button type="button" class="error-recover" @click="goToRoot">Back to root</button>
      </p>
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

        <div v-else class="details-grid" role="table" aria-label="Directory contents">
          <div class="details-header details-row" role="row">
            <span role="columnheader">Name</span>
            <span role="columnheader">Size</span>
            <span role="columnheader">Modified</span>
            <span class="actions-header" role="columnheader">Actions</span>
          </div>

          <div
            v-for="entry in entries"
            :key="`${entry.kind}:${entry.name}`"
            class="details-row"
            :class="{ 'is-confirming': confirmingDelete === entry.name }"
            role="row"
          >
            <div class="name-cell" role="cell">
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
            <span class="size-cell" role="cell">{{ formatSize(entry.sizeBytes) }}</span>
            <span class="date-cell" role="cell">{{ formatModified(entry.modifiedAt) }}</span>
            <span class="actions-cell" role="cell">
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
              <template v-if="entry.kind === 'file' && entry.canDelete">
                <button
                  v-if="confirmingDelete !== entry.name"
                  type="button"
                  class="icon-action delete-action"
                  :aria-label="`Delete ${entry.name}`"
                  title="Delete"
                  @click="startDeleteConfirmation(entry.name)"
                >
                  <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                    <path
                      d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z"
                    />
                  </svg>
                </button>
                <template v-else>
                  <button type="button" class="confirm-delete" @click="confirmDelete(entry)">
                    Delete?
                  </button>
                  <button type="button" class="cancel-delete" @click="cancelDeleteConfirmation">
                    No
                  </button>
                </template>
              </template>
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
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
  min-width: 0;
}

.user-menu {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
  flex-shrink: 0;
}

.user-name {
  font-size: 0.82rem;
  color: #243242;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 12rem;
}

.user-logout {
  font-size: 0.78rem;
  color: #0b57a1;
  border: 1px solid #d8e2ee;
  border-radius: 0.3rem;
  padding: 0.22rem 0.55rem;
  text-decoration: none;
  background: #f7fafc;
  white-space: nowrap;
}

.user-logout:hover {
  text-decoration: underline;
  background: #eef4fa;
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

/* The delete action reads as destructive: a red accent instead of the blue
   link accent the download action carries. */
.delete-action {
  color: #c23b32;
  border-color: #e8c9c4;
}

/* The inline confirmation needs two buttons; the actions track is one icon
   wide otherwise. A confirming row widens the track so the buttons fit
   without pushing the other columns out of the panel. */
.details-row.is-confirming {
  grid-template-columns: minmax(8rem, 1fr) 9rem 11.5rem auto;
}

.confirm-delete,
.cancel-delete {
  border-radius: 0.3rem;
  font-size: 0.75rem;
  padding: 0.2rem 0.45rem;
  cursor: pointer;
  white-space: nowrap;
}

.confirm-delete {
  border: 1px solid #c23b32;
  background: #ffffff;
  color: #c23b32;
}

.confirm-delete:hover,
.confirm-delete:focus-visible {
  background: #c23b32;
  color: #ffffff;
}

.cancel-delete {
  border: 1px solid #d8e2ee;
  background: #f7fafc;
  color: #243242;
}

.cancel-delete:hover {
  background: #eef4fa;
}

.empty-state,
.error-panel p,
.notice-panel p {
  padding: 0.8rem;
  margin: 0;
  font-size: 0.88rem;
}

.notice-panel h2 {
  font-size: 0.95rem;
  margin: 0.8rem 0 0;
}

/* The error panel reads as an error: a red accent stripe, an icon beside the
   heading, and a recovery action instead of a bare message. */
.error-panel {
  border-left: 0.22rem solid #c23b32;
}

.error-panel h2 {
  font-size: 0.95rem;
  margin: 0.8rem 0 0;
}

.error-heading {
  display: flex;
  align-items: center;
  gap: 0.45rem;
  padding-inline: 0.8rem;
  padding-top: 0.8rem;
}

/* The heading's top margin from the plain-error style would double-count
   the flex container's padding and shift the text off the icon's center
   line, so it resets here. */
.error-heading h2 {
  margin: 0;
}

.error-icon {
  width: 1.05rem;
  height: 1.05rem;
  flex-shrink: 0;
  fill: #c23b32;
}

/* Scoped under .error-panel so the reset outranks the .error-panel p
   rule (class + element beats the bare class in the cascade). */
.error-panel .error-actions {
  padding: 0.8rem;
  padding-top: 0;
}

.error-recover {
  border: 1px solid #c23b32;
  border-radius: 0.35rem;
  background: #ffffff;
  padding: 0.35rem 0.75rem;
  color: #c23b32;
  font-size: 0.85rem;
  cursor: pointer;
}

.error-recover:hover,
.error-recover:focus-visible {
  background: #c23b32;
  color: #ffffff;
}

@media (max-width: 640px) {
  .page-shell {
    padding-inline: 0.55rem;
  }

  /* The user menu keeps its place in the header on a phone viewport: only
     the display name is allowed to shrink, and the email tooltip carries
     the rest. */
  .user-name {
    max-width: 7rem;
    font-size: 0.78rem;
  }

  .user-logout {
    font-size: 0.75rem;
    padding: 0.2rem 0.45rem;
  }

  .breadcrumb-link,
  .breadcrumb-separator {
    font-size: 0.78rem;
  }

  /* The four-column table cannot fit a phone viewport: its minimum track
     widths exceed the available space and push the actions cell outside
     the panel. Stack each row instead: name and action on the first line,
     size and modified on the second. The header row is visually hidden
     rather than removed so the table roles keep their column labels in
     the accessibility tree. */
  .details-header {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .details-row:not(.details-header) {
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-areas:
      'name actions'
      'size date';
    column-gap: 0.8rem;
    row-gap: 0.15rem;
    align-items: baseline;
    min-height: 0;
    padding: 0.5rem 0.6rem;
  }

  .name-cell {
    grid-area: name;
  }

  .actions-cell {
    grid-area: actions;
    align-self: center;
  }

  .size-cell {
    grid-area: size;
    font-size: 0.75rem;
  }

  .date-cell {
    grid-area: date;
    font-size: 0.75rem;
  }
}
</style>
