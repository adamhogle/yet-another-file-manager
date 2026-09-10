import fs from 'node:fs/promises';
import path from 'node:path';
import YAML from 'yaml';

const root = process.cwd();
const specPath = path.join(root, 'api/openapi.yaml');
const outputPath = path.join(root, 'frontend/src/lib/api/generated/client.js');

function toFunctionName(method, rawPath, operationId) {
  if (operationId) {
    return operationId.replace(/[^a-zA-Z0-9]/g, '');
  }

  const pathPart = rawPath
    .split('/')
    .filter(Boolean)
    .map((part) => part.replace(/[{}]/g, ''))
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join('');

  return `${method.toLowerCase()}${pathPart}`;
}

function renderOperation(method, rawPath, operation) {
  const functionName = toFunctionName(method, rawPath, operation.operationId);
  const pathTemplate = rawPath.replace(/`/g, '');
  const urlPath = pathTemplate.replace(
    /\{(\w+)\}/g,
    (_, name) => `\${encodeURIComponent(params.${name})}`
  );
  const hasPathParams = /\{(\w+)\}/.test(rawPath);
  const signature = hasPathParams
    ? 'baseUrl = DEFAULT_BASE_URL, params = {}, query = {}, init = {}'
    : 'baseUrl = DEFAULT_BASE_URL, query = {}, init = {}';
  const responseBody = rawPath.includes('/download') ? 'response.blob()' : 'response.json()';

  return `export async function ${functionName}(${signature}) {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') {
      continue;
    }

    searchParams.set(key, String(value));
  }

  const queryString = searchParams.toString();
  const response = await fetch(\`\${baseUrl}${urlPath}\${queryString ? \`?\${queryString}\` : ''}\`, {
    ...init,
    method: '${method.toUpperCase()}',
    headers: {
      ...(init.headers ?? {})
    }
  });

  if (!response.ok) {
    let message = \`Request failed: ${method.toUpperCase()} ${pathTemplate} -> \${response.status}\`;
    try {
      const body = await response.json();
      if (body && typeof body.message === 'string' && body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON body: keep the fallback message.
    }
    throw new Error(message);
  }

  return ${responseBody};
}
`;
}

async function main() {
  const rawSpec = await fs.readFile(specPath, 'utf8');
  const spec = YAML.parse(rawSpec);

  if (!spec?.paths || typeof spec.paths !== 'object') {
    throw new Error('OpenAPI spec is missing paths object');
  }

  const seenNames = new Map();
  const operations = [];

  for (const [rawPath, pathItem] of Object.entries(spec.paths)) {
    for (const method of ['get', 'post', 'put', 'patch', 'delete']) {
      const operation = pathItem?.[method];
      if (!operation) {
        continue;
      }

      const functionName = toFunctionName(method, rawPath, operation.operationId);
      const previousOwner = seenNames.get(functionName);
      if (previousOwner) {
        throw new Error(
          `Duplicate generated function name '${functionName}' (${previousOwner} and ${method.toUpperCase()} ${rawPath})`
        );
      }
      seenNames.set(functionName, `${method.toUpperCase()} ${rawPath}`);

      operations.push(renderOperation(method, rawPath, operation));
    }
  }

  const content = `// AUTO-GENERATED FILE. DO NOT EDIT.\n// Source: api/openapi.yaml\n\nexport const DEFAULT_BASE_URL = '';\n\n${operations.join('\n')}`;

  await fs.mkdir(path.dirname(outputPath), { recursive: true });
  await fs.writeFile(outputPath, content, 'utf8');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
