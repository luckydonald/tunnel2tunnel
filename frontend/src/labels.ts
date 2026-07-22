import type { AccessRule, Friendship } from '@/api/friends'
import type { BanScopeType, TarpitMethod } from '@/api/admin'

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

export const tarpitMethodLabel: Record<TarpitMethod, string> = {
  banner_drip: 'Banner drip (endlessh-style)',
  slow_auth:   'Slow auth drip-feed',
  fake_shell:  'Fake interactive shell',
}

export const tarpitMethodOptions = (
  Object.entries(tarpitMethodLabel) as [TarpitMethod, string][]
).map(([value, label]) => ({ value, label }))

export const banScopeTypeLabel: Record<BanScopeType, string> = {
  peer_ip: 'IP address',
  user:    'User account',
}

export const banScopeTypeOptions = (
  Object.entries(banScopeTypeLabel) as [BanScopeType, string][]
).map(([value, label]) => ({ value, label }))

export const failReasonLabel: Record<string, string> = {
  'password auth not supported': 'Password auth attempted (unsupported)',
  'unsupported auth method':     'Unsupported auth method attempted',
  'unknown key':                 'Unknown SSH key',
  'key expired':                 'SSH key expired (revoked)',
  'key expired (valid_until)':   'SSH key expired (valid_until)',
  'entity not found':            'Entity not found',
  'entity expired':              'Entity expired',
  'ip blocked by whitelist':     'Blocked by IP whitelist',
  'ip banned':                   'IP banned',
  'user banned':                 'User banned',
  'rate limited':                'Rate limited',
  'tarpitted - gave up':         'Tarpitted (client gave up)',
  'unspecified failure':         'Unspecified failure',
}

export const successReasonLabel: Record<string, string> = {
  'correct login':    'Correct login',
  'admin reset ban':  'Admin reset ban',
}
