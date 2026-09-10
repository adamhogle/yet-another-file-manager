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
  const urlPath = rawPath.replace(/`/g, '');
  const responseBody = rawPath.includes('/download') ? 'response.blob()' : 'response.json()';

  return `export async function ${functionName}(baseUrl = DEFAULT_BASE_URL, query = {}, init = {}) {\n  const searchParams = new URLSearchParams();\n  for (const [key, value] of Object.entries(query)) {\n    if (value === undefined || value === null || value === '') {\n      continue;\n    }\n\n    searchParams.set(key, String(value));\n  }\n\n  const queryString = searchParams.toString();\n  const response = await fetch(\`${'${baseUrl}'}${urlPath}${'${queryString ? `?${queryString}` : ""}'}\`, {\n    ...init,\n    method: '${method.toUpperCase()}',\n    headers: {\n      ...(init.headers ?? {})\n    }\n  });\n\n  if (!response.ok) {\n    throw new Error(\`Request failed: ${method.toUpperCase()} ${urlPath} -> ${'${response.status}'}\`);\n  }\n\n  return ${responseBody};\n}\n`;
}

async function main() {
  const rawSpec = await fs.readFile(specPath, 'utf8');
  const spec = YAML.parse(rawSpec);

  if (!spec?.paths || typeof spec.paths !== 'object') {
    throw new Error('OpenAPI spec is missing paths object');
  }

  const operations = [];

  for (const [rawPath, pathItem] of Object.entries(spec.paths)) {
    for (const method of ['get', 'post', 'put', 'patch', 'delete']) {
      const operation = pathItem?.[method];
      if (!operation) {
        continue;
      }

      operations.push(renderOperation(method, rawPath, operation));
    }
  }

  const content = `// AUTO-GENERATED FILE. DO NOT EDIT.\n// Source: api/openapi.yaml\n\nexport const DEFAULT_BASE_URL = 'http://localhost:8080';\n\n${operations.join('\n')}`;

  await fs.mkdir(path.dirname(outputPath), { recursive: true });
  await fs.writeFile(outputPath, content, 'utf8');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
