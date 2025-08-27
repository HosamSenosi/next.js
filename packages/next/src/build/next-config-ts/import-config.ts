import { parentPort, workerData } from 'worker_threads'
import { pathToFileURL } from 'url'
import { register } from 'module'
import { join } from 'path'

async function importConfig() {
  try {
    register(pathToFileURL(join(import.meta.dirname, 'loader.js')).href, {
      parentURL: pathToFileURL(workerData.cwd).href,
      data: {
        cwd: workerData.cwd,
        compilerOptions: workerData.compilerOptions,
      },
    })

    const configModule = await import(
      pathToFileURL(workerData.nextConfigPath).href
    )

    parentPort?.postMessage({
      success: true,
      config: configModule,
    })
  } catch (error: any) {
    parentPort?.postMessage({
      success: false,
      error: error.message || 'Unknown error occurred while importing config',
      stack: error.stack || '',
      code: error.code || 'CONFIG_IMPORT_ERROR',
    })
  }
}

importConfig()
