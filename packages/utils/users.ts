// noinspection JSUnusedGlobalSymbols

import type { User } from './types'

type UserRoleLike = Pick<User, 'role'> | null | undefined

export const getUserLink = (user: Pick<User, 'username'>) => {
  return `/user/${user.username}`
}

export const isStaff = (user: UserRoleLike) => {
  return user && STAFF_ROLES.includes(user.role)
}

export const isAdmin = (user: UserRoleLike) => {
  return user && user.role === 'admin'
}

export const STAFF_ROLES: User['role'][] = ['moderator', 'admin']
