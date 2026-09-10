import fs from 'node:fs/promises';
import path from 'node:path';
import YAML from 'yaml';

const root = process.cwd();
const specPath = path.join(root, 'api/openapi.yaml');
const outputPath = path.join(root, 'frontend/src/lib/api/generated/client.ts');

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

function refToSchemaName(ref) {
  const match = /^#\/components\/schemas\/([\w-]+)$/.exec(ref);
  if (!match) {
    throw new Error(`Unsupported $ref: ${ref}`);
  }

  return match[1];
}

function isPlainIdentifier(name) {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name);
}

function renderBaseType(node) {
  if (node.$ref) {
    return `${refToSchemaName(node.$ref)}Schema`;
  }

  const typeEntries = Array.isArray(node.type)
    ? node.type.filter((entry) => entry !== 'null')
    : [node.type];
  if (typeEntries.length !== 1) {
    throw new Error(`Unsupported schema type: ${JSON.stringify(node.type)}`);
  }
  const type = typeEntries[0];

  if (type === 'string') {
    if (Array.isArray(node.enum)) {
      return `z.enum([${node.enum.map((value) => JSON.stringify(value)).join(', ')}])`;
    }

    return 'z.string()';
  }

  if (type === 'integer' || type === 'number') {
    let chain = 'z.number()';
    if (node.format === 'int32' || node.format === 'int64') {
      chain += '.int()';
    }
    if (typeof node.minimum === 'number') {
      chain += `.min(${node.minimum})`;
    }

    return chain;
  }

  if (type === 'array') {
    return `z.array(${renderBaseType(node.items ?? {})})`;
  }

  if (type === 'object') {
    const required = new Set(Array.isArray(node.required) ? node.required : []);
    const properties = Object.entries(node.properties ?? {}).map(([propertyName, propertyNode]) => {
      const key = isPlainIdentifier(propertyName) ? propertyName : JSON.stringify(propertyName);
      return `${key}: ${renderPropertyType(propertyNode, required.has(propertyName))}`;
    });

    return `z.object({ ${properties.join(', ')} })`;
  }

  throw new Error(`Unsupported schema type: ${JSON.stringify(node.type)}`);
}

function renderPropertyType(node, isRequired) {
  let type = renderBaseType(node);

  if (Array.isArray(node.type) && node.type.includes('null')) {
    type += '.nullable()';
  }

  if (!isRequired) {
    type += '.optional()';
  }

  return type;
}

function collectSchemaRefs(node, refs) {
  if (!node || typeof node !== 'object') {
    return;
  }

  if (typeof node.$ref === 'string') {
    refs.push(node.$ref);
  }

  for (const value of Object.values(node)) {
    if (value && typeof value === 'object') {
      collectSchemaRefs(value, refs);
    }
  }
}

function topologicallySortSchemas(schemasByName) {
  const emitted = new Set();
  const inProgress = new Set();
  const ordered = [];

  function dependenciesOf(schema) {
    const refs = [];
    collectSchemaRefs(schema, refs);
    const dependencies = new Set();
    for (const ref of refs) {
      const name = refToSchemaName(ref);
      if (schemasByName.has(name)) {
        dependencies.add(name);
      }
    }

    return [...dependencies];
  }

  function visit(name) {
    if (emitted.has(name)) {
      return;
    }

    if (inProgress.has(name)) {
      throw new Error(
        `Recursive schema reference detected at '${name}' (components.schemas must be acyclic; cyclic schemas are not supported by the generated client)`
      );
    }
    inProgress.add(name);

    for (const dependency of dependenciesOf(schemasByName.get(name))) {
      visit(dependency);
    }

    inProgress.delete(name);
    emitted.add(name);
    ordered.push(name);
  }

  for (const name of schemasByName.keys()) {
    visit(name);
  }

  return ordered;
}

function renderSchemaSection(schemasByName) {
  return topologicallySortSchemas(schemasByName)
    .map((name) => {
      const schema = schemasByName.get(name);
      return [
        `export const ${name}Schema = ${renderBaseType(schema)};`,
        `export type ${name} = z.infer<typeof ${name}Schema>;`
      ].join('\n');
    })
    .join('\n\n');
}

function resolveResponseSchema(operation, method, rawPath) {
  const response = operation.responses?.['200'];
  if (!response) {
    throw new Error(`Operation ${method.toUpperCase()} ${rawPath} is missing a 200 response`);
  }

  const jsonResponse = Object.entries(response.content ?? {}).find(([mediaType]) =>
    mediaType.endsWith('json')
  );

  if (!jsonResponse) {
    // No JSON content: a binary response with no schema to validate against.
    return null;
  }

  const schema = jsonResponse[1]?.schema;
  if (!schema?.$ref) {
    throw new Error(
      `Operation ${method.toUpperCase()} ${rawPath} returns JSON content without a $ref schema; the generated client needs a named schema`
    );
  }

  return refToSchemaName(schema.$ref);
}

function renderOperation(method, rawPath, operation) {
  const functionName = toFunctionName(method, rawPath, operation.operationId);
  const pathTemplate = rawPath.replace(/`/g, '');
  const urlPath = pathTemplate.replace(
    /\{(\w+)\}/g,
    (_, name) => `\${encodeURIComponent(params.${name})}`
  );
  const hasPathParams = /\{(\w+)\}/.test(rawPath);
  const responseSchema = resolveResponseSchema(operation, method, rawPath);
  const queryType = 'Record<string, string | number | boolean | null | undefined>';
  const signature = hasPathParams
    ? `baseUrl: string = DEFAULT_BASE_URL, params: Record<string, string | number | boolean> = {}, query: ${queryType} = {}, init: RequestInit = {}`
    : `baseUrl: string = DEFAULT_BASE_URL, query: ${queryType} = {}, init: RequestInit = {}`;
  const typeName = responseSchema ?? 'Blob';
  const successReturn = responseSchema
    ? `return ${responseSchema}Schema.parse(await response.json());`
    : 'return response.blob();';

  return `export async function ${functionName}(${signature}): Promise<${typeName}> {
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
      const body = PublicErrorResponseSchema.parse(await response.json());
      if (body.message) {
        message = body.message;
      }
    } catch {
      // Non-JSON or non-conforming body: keep the fallback message.
    }
    throw new Error(message);
  }

  ${successReturn}
}
`;
}

async function main() {
  const rawSpec = await fs.readFile(specPath, 'utf8');
  const spec = YAML.parse(rawSpec);

  if (!spec?.paths || typeof spec.paths !== 'object') {
    throw new Error('OpenAPI spec is missing paths object');
  }

  const schemasByName = new Map(Object.entries(spec.components?.schemas ?? {}));
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

  const hasSchemas = schemasByName.size > 0;
  const zodImport = hasSchemas ? "import { z } from 'zod';\n\n" : '';
  const content = `// AUTO-GENERATED FILE. DO NOT EDIT.\n// Source: api/openapi.yaml\n\n${zodImport}export const DEFAULT_BASE_URL = '';\n\n${renderSchemaSection(schemasByName)}\n\n${operations.join('\n')}`;

  await fs.mkdir(path.dirname(outputPath), { recursive: true });
  await fs.writeFile(outputPath, content, 'utf8');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
