declare module '*.vue' {
  import type { DefineComponent } from 'vue'

  const component: DefineComponent<
    Record<string, unknown>,
    Record<string, unknown>,
    unknown
  >
  export default component
}

declare module 'fuse.js/dist/fuse.basic' {
  export { default } from 'fuse.js'
}

declare module '@modrinth/api-client' {
  import type { EnvironmentMigrationReviewStatus, EnvironmentV3 } from '@modrinth/utils'

  export namespace Labrinth {
    namespace Projects {
      namespace v2 {
        export interface Project {
          id: string
        }
      }

      namespace v3 {
        export type Environment = EnvironmentV3

        export interface Project {
          id: string
          project_types: string[]
          environment?: Environment[]
          side_types_migration_review_status: EnvironmentMigrationReviewStatus | null
          [key: string]: unknown
        }
      }
    }

    namespace Tags {
      namespace v2 {
        export interface GameVersion {
          version: string
          version_type: 'release' | 'snapshot' | 'alpha' | 'beta'
          date: string
          major: boolean
        }

        export interface Loader {
          name: string
          icon?: string
          supported_project_types: string[]
        }

        export interface Category {
          name: string
          header: string
          project_type: string
          icon?: string
        }
      }
    }
  }

  export namespace Archon {
    namespace Backups {
      namespace v1 {
        export type BackupState = string
      }
    }

    namespace Servers {
      namespace v0 {
        export type Server = Record<string, unknown>
      }
    }

    namespace Websocket {
      namespace v0 {
        export type PowerState = string
        export type FilesystemOperation = Record<string, unknown>
        export type QueuedFilesystemOp = Record<string, unknown>
      }
    }
  }

  export interface AbstractModrinthClient {
    labrinth: {
      projects_v3: {
        edit: (
          projectId: string,
          data: {
            environment?: Labrinth.Projects.v3.Environment
            side_types_migration_review_status?: EnvironmentMigrationReviewStatus
          },
        ) => Promise<unknown>
      }
    }
  }
}

declare module '@modrinth/api-client/src/modules/types' {
  export { Labrinth } from '@modrinth/api-client'
}

type NonEmptyObject<T> = T extends object
  ? keyof T extends never
    ? never
    : T
  : never

type ValidKeys<T> =
  NonEmptyObject<T> extends infer O
    ? O extends object
      ? keyof O
      : never
    : never
