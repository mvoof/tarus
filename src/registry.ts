import { RegistryEntry, SymbolType, LanguageId, FilePath } from './types';

export const registry = new Map<
  LanguageId,
  {
    command: Map<string, RegistryEntry>;
    event: Map<string, RegistryEntry>;
  }
>();

export function getFrontendLanguageId(
  type: SymbolType,
  name: string
): LanguageId | null {
  for (const [langId, maps] of registry.entries()) {
    if (langId !== 'rust') {
      if (maps[type].has(name)) {
        return langId;
      }
    }
  }

  return null;
}

/** Register a paired symbol */
export function registerPair(
  name: string,
  sourceLocation: FilePath,
  sourceLanguage: LanguageId,
  targetLanguage: LanguageId,
  type: SymbolType,
  sourceOffset: number
) {
  if (!registry.has(sourceLanguage)) {
    registry.set(sourceLanguage, { command: new Map(), event: new Map() });
  }

  const sourceEntry: RegistryEntry = {
    location: sourceLocation,
    language: sourceLanguage,
    offset: sourceOffset,
    counterpart: { language: targetLanguage, type, name, offset: -1 },
  };

  registry.get(sourceLanguage)![type].set(name, sourceEntry);
}

/** Update counterpart location and offset when found */
export function updateCounterpart(
  name: string,
  location: FilePath,
  language: LanguageId,
  type: SymbolType,
  offset: number
) {
  const langMap = registry.get(language);

  if (!langMap) return;

  const entry = langMap[type].get(name);

  if (entry) {
    entry.location = location;
    entry.offset = offset;

    const targetMap = registry.get(entry.counterpart.language);
    const targetEntry = targetMap?.[entry.counterpart.type]?.get(
      entry.counterpart.name
    );

    if (targetEntry) {
      targetEntry.counterpart.offset = offset;
    }
  }
}

/** Get counterpart location and offset */
export function getCounterpartInfo(
  language: LanguageId,
  type: SymbolType,
  name: string
): { location: FilePath; offset: number } | null {
  const langMap = registry.get(language);

  if (!langMap) return null;

  const entry = langMap[type].get(name);

  if (!entry) return null;

  const {
    language: targetLang,
    type: targetType,
    name: targetName,
  } = entry.counterpart;
  const targetMap = registry.get(targetLang);

  if (!targetMap) return null;

  const targetEntry = targetMap[targetType].get(targetName);

  if (targetEntry?.location) {
    return {
      location: targetEntry.location,
      offset: targetEntry.offset,
    };
  }

  return null;
}

/** Clear all */
export function clearRegistry() {
  registry.clear();
}

/** Clear all entries associated with a specific file path */
export function clearRegistryForFile(filePath: string) {
  for (const [_, maps] of registry.entries()) {
    // Clear command entries
    for (const [name, entry] of maps.command.entries()) {
      if (entry.location === filePath) {
        maps.command.delete(name);
      }
    }

    // Clear event entries
    for (const [name, entry] of maps.event.entries()) {
      if (entry.location === filePath) {
        maps.event.delete(name);
      }
    }
  }
}
