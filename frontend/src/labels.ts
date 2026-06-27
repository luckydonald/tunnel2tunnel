import type { AccessRule, Friendship } from '@/api/friends'

export const subjectTypeLabel: Record<AccessRule['subject_type'], string> = {
  public_lite:        'Anyone (no account required)',
  all_mine:           'All my own entities',
  all_user_entities:  'All entities of a specific user',
  entity:             'One specific entity',
}

export const subjectTypeOptions = (
  Object.entries(subjectTypeLabel) as [AccessRule['subject_type'], string][]
).map(([value, label]) => ({ value, label }))

export const visibilityGrantLabel: Record<Friendship['visibility_grant'], string> = {
  none:    'Hidden (no visibility)',
  clients: 'Clients only',
  servers: 'Servers only',
  all:     'All entities',
}

export const visibilityGrantOptions = (
  Object.entries(visibilityGrantLabel) as [Friendship['visibility_grant'], string][]
).map(([value, label]) => ({ value, label }))

export const friendshipStatusLabel: Record<Friendship['status'], string> = {
  pending:  'Pending',
  accepted: 'Accepted',
  declined: 'Declined',
}

export const entityTypeLabel: Record<'server' | 'client', string> = {
  server: 'Server',
  client: 'Client',
}
