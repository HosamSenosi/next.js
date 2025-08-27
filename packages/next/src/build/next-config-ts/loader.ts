import type {
  InitializeHook,
  LoadHook,
  ModuleFormat,
  ResolveHook,
} from 'module'
import type { Options as SWCOptions } from '@swc/core'
import { extname, join } from 'path'
import { transform } from '../swc/index.js'
import { readFile } from 'fs/promises'
import { fileURLToPath } from 'url'
import type { CompilerOptions } from 'typescript'

const tsExts = new Set(['.ts', '.mts', '.cts'])
const localContext = new Map<string, any>()

export const initialize: InitializeHook<{
  cwd: string
  compilerOptions: CompilerOptions
}> = async ({ cwd, compilerOptions }) => {
  localContext.set('cwd', cwd)
  localContext.set('compilerOptions', compilerOptions)
}

export const resolve: ResolveHook = async (specifier, context, nextResolve) => {
  // next.config.* is imported from internal "import-config.js" file,
  // so we expect the parentURL is available.
  if (!context.parentURL) {
    return nextResolve(specifier, context)
  }

  // e.g. "next", "react", etc.
  // Packages that look like bare specifiers depending on the "baseUrl"
  // should already be resolved path by SWC.
  if (!specifier.startsWith('.') && !URL.canParse(specifier)) {
    return nextResolve(specifier, context)
  }

  const ext = extname(specifier)
  // Node.js resolver can take care of the rest of the non-TS files.
  if (!tsExts.has(ext)) {
    return nextResolve(specifier, context)
  }

  return {
    format: 'typescript' as ModuleFormat,
    shortCircuit: true,
    url: new URL(specifier, context.parentURL).href,
  }
}

export const load: LoadHook = async (url, context, nextLoad) => {
  if (context.format !== ('typescript' as ModuleFormat)) {
    return nextLoad(url, context)
  }

  const cwd = localContext.get('cwd')
  if (!cwd) {
    throw new Error(
      'The "cwd" value was not passed to the loader from the registration. This is a bug in Next.js.'
    )
  }
  const rawSource = await readFile(fileURLToPath(url), 'utf-8')
  const compilerOptions = localContext.get('compilerOptions') as CompilerOptions
  const { code } = await transform(rawSource, {
    jsc: {
      parser: {
        syntax: 'typescript',
      },
      ...(compilerOptions?.paths ? { paths: compilerOptions.paths } : {}),
      // SWC requires `baseUrl` to be passed when `paths` are used.
      // Also, `baseUrl` must be absolute.
      ...(compilerOptions?.baseUrl
        ? { baseUrl: join(cwd, compilerOptions.baseUrl) }
        : {}),
      experimental: {
        keepImportAttributes: true,
        // Without this option, `assert` assertion also transpiles to `with` attribute,
        // which will throw if in Node.js version that does not support `with`.
        // Switch from Import Assertions to Import Attributes held at v21.0.0, v20.10.0, v18.20.0.
        emitAssertForImportAttributes: true,
      },
    },
    env: {
      targets: {
        // TODO: Bump to v20 when v18 EOL.
        // The value may be missing in other runtimes, so fallback to
        // the minimum Node.js version.
        node: process?.versions?.node ?? '18.18.0',
      },
    },
  } satisfies SWCOptions)

  return {
    format: 'module',
    shortCircuit: true,
    source: code,
  }
}
