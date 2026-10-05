// Back-compat shim: the Anthropic key/model accessors the original app used. New code uses chat/providers.ts `store`.
import { store } from './chat/providers';

export const MODELS = [
  { id: 'claude-opus-5-5', label: 'CLAUDE OPUS 5.5 (DEFAULT)' },
  { id: 'claude-sonnet-5-5', label: 'CLAUDE SONNET 5.5' },
];

export const settings = {
  get apiKey(): string { return store.key('anthropic'); },
  set apiKey(v: string) { store.setKey('anthropic', v); },
  get model(): string { return store.model('anthropic'); },
  set model(v: string) { store.setModel('anthropic', v); },
};
