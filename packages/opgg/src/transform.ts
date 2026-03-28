import { nanoid } from 'nanoid';
import type {
  OpggPageData,
  OpggRunePage,
  OpggItemBuilds,
  GameMode,
  LcuRune,
  LcuItemBuild,
  LcuBlock,
  LcuItem,
  LcuBuildSection,
} from './types.js';

const SOURCE_NAME = 'op.gg';
const SUMMONERS_RIFT_MAP_ID = 11;
const HOWLING_ABYSS_MAP_ID = 12;

/** Get the map ID(s) for a given game mode */
function getMapIds(mode: GameMode): number[] {
  switch (mode) {
    case 'aram':
    case 'aram-mayhem':
      return [HOWLING_ABYSS_MAP_ID];
    case 'urf':
    case 'ranked':
    default:
      return [SUMMONERS_RIFT_MAP_ID];
  }
}

/** Get a display label for the mode */
function getModeLabel(mode: GameMode): string {
  switch (mode) {
    case 'aram':
      return 'ARAM';
    case 'urf':
      return 'URF';
    case 'aram-mayhem':
      return 'ARAM Mayhem';
    case 'ranked':
    default:
      return '';
  }
}

/**
 * Transform an OP.GG rune page into the LCU Rune format.
 * This produces the exact JSON that gets POSTed to /lol-perks/v1/pages.
 */
function transformRunePage(
  runePage: OpggRunePage,
  champion: string,
  position: string,
  label: string,
): LcuRune {
  const winRateStr = `${(runePage.win_rate * 100).toFixed(2)}%`;

  return {
    uuid: nanoid(10),
    alias: champion,
    name: `[OP.GG] ${champion} - ${label} (${winRateStr} WR)`,
    position,
    pickCount: runePage.play,
    winRate: winRateStr,
    primaryStyleId: runePage.importClientData.primaryStyleId,
    subStyleId: runePage.importClientData.subStyleId,
    selectedPerkIds: runePage.importClientData.selectedPerkIds,
    score: null,
    type: '',
  };
}

/**
 * Transform 2 OP.GG rune pages into 2 LCU Rune objects:
 * - First: "Most Popular" (highest pick rate)
 * - Second: "Highest Win Rate" (other rune page, often higher WR)
 *
 * For ARAM Mayhem mode, returns empty array (no runes stored).
 */
export function transformRunes(
  runePages: OpggRunePage[],
  champion: string,
  position: string,
  mode: GameMode = 'ranked',
): LcuRune[] {
  // ARAM Mayhem has no runes
  if (mode === 'aram-mayhem') return [];

  if (runePages.length === 0) return [];

  const runes: LcuRune[] = [];

  // Sort by pick_rate descending to identify most popular
  const sorted = [...runePages].sort((a, b) => b.pick_rate - a.pick_rate);

  // First rune page = most popular
  runes.push(transformRunePage(sorted[0], champion, position, 'Most Popular'));

  // Second rune page = highest win rate (from the remaining)
  if (sorted.length > 1) {
    // Find the one with highest win rate that isn't the most popular
    const remaining = sorted.slice(1);
    const highestWR = remaining.sort((a, b) => b.win_rate - a.win_rate)[0];
    runes.push(transformRunePage(highestWR, champion, position, 'Highest Win Rate'));
  }

  return runes;
}

/**
 * Build an LcuItem from a Riot item ID.
 */
function makeItem(itemId: number): LcuItem {
  return { id: String(itemId), count: 1 };
}

/**
 * Transform OP.GG item builds into the Riot item set JSON format.
 * This produces JSON files that LoL client reads from Config/Champions/{alias}/Recommended/
 *
 * For ARAM Mayhem mode, win rate stats are omitted from block labels.
 */
export function transformItemBuilds(
  itemBuilds: OpggItemBuilds,
  champion: string,
  position: string,
  championId?: number,
  mode: GameMode = 'ranked',
): LcuItemBuild[] {
  const builds: LcuItemBuild[] = [];
  const mapIds = getMapIds(mode);
  const modeLabel = getModeLabel(mode);
  const modeTag = modeLabel ? ` ${modeLabel}` : '';
  const hideWR = mode === 'aram-mayhem';

  // --- Build 1: Most Popular Full Build ---
  // Uses the most popular option from each category
  const blocks: LcuBlock[] = [];

  // Starter Items - top pick
  if (itemBuilds.starterItems.length > 0) {
    const top = itemBuilds.starterItems[0];
    blocks.push({
      type: hideWR
        ? `Starter Items :: ${top.play} Games`
        : `Starter Items :: ${top.win_rate}% WR - ${top.play} Games`,
      items: top.items.map((item) => makeItem(item.id)),
    });
  }

  // All starter item options (if more than 1)
  if (itemBuilds.starterItems.length > 1) {
    const allItems: LcuItem[] = [];
    const seen = new Set<number>();
    for (const row of itemBuilds.starterItems) {
      for (const item of row.items) {
        if (!seen.has(item.id)) {
          seen.add(item.id);
          allItems.push(makeItem(item.id));
        }
      }
    }
    if (allItems.length > (itemBuilds.starterItems[0]?.items.length ?? 0)) {
      blocks.push({
        type: 'Starter Item Options',
        items: allItems,
      });
    }
  }

  // Boots - all options
  if (itemBuilds.boots.length > 0) {
    blocks.push({
      type: hideWR
        ? 'Boots'
        : `Boots :: ${itemBuilds.boots[0].win_rate}% WR`,
      items: itemBuilds.boots.map((b) => makeItem(b.item.id)),
    });
  }

  // Core Build #1 (most popular)
  if (itemBuilds.coreBuilds.length > 0) {
    const top = itemBuilds.coreBuilds[0];
    blocks.push({
      type: hideWR
        ? `Core Build :: ${top.play} Games`
        : `Core Build :: ${top.win_rate}% WR - ${top.play} Games`,
      items: top.items.map((item) => makeItem(item.id)),
    });
  }

  // Core Build alternatives
  for (let i = 1; i < Math.min(itemBuilds.coreBuilds.length, 4); i++) {
    const build = itemBuilds.coreBuilds[i];
    blocks.push({
      type: hideWR
        ? `Core Build #${i + 1} :: ${build.play} Games`
        : `Core Build #${i + 1} :: ${build.win_rate}% WR - ${build.play} Games`,
      items: build.items.map((item) => makeItem(item.id)),
    });
  }

  // 4th Item Options
  if (itemBuilds.fourthItems.length > 0) {
    blocks.push({
      type: '4th Item Options',
      items: itemBuilds.fourthItems.map((row) => makeItem(row.item.id)),
    });
  }

  // 5th Item Options
  if (itemBuilds.fifthItems.length > 0) {
    blocks.push({
      type: '5th Item Options',
      items: itemBuilds.fifthItems.map((row) => makeItem(row.item.id)),
    });
  }

  // 6th Item Options
  if (itemBuilds.sixthItems.length > 0) {
    blocks.push({
      type: '6th Item Options',
      items: itemBuilds.sixthItems.map((row) => makeItem(row.item.id)),
    });
  }

  // Only push build 1 if it has any blocks
  if (blocks.length > 0) {
    builds.push({
      title: `[OP.GG] ${champion}${modeTag} - ${position || 'Build'}`,
      associatedMaps: mapIds,
      associatedChampions: championId ? [championId] : [],
      blocks,
      map: 'any',
      mode: 'any',
      preferredItemSlots: [],
      sortrank: 0,
      startedFrom: 'heuristic',
      type: 'custom',
    });
  }

  // --- Build 2: Highest Win Rate Build ---
  // Uses the highest WR option from each category
  const wrBlocks: LcuBlock[] = [];

  // Starter with highest WR
  if (itemBuilds.starterItems.length > 0) {
    const bestWR = [...itemBuilds.starterItems].sort(
      (a, b) => b.win_rate - a.win_rate,
    )[0];
    wrBlocks.push({
      type: hideWR
        ? `Starter Items :: ${bestWR.play} Games`
        : `Starter Items :: ${bestWR.win_rate}% WR - ${bestWR.play} Games`,
      items: bestWR.items.map((item) => makeItem(item.id)),
    });
  }

  // Boots with highest WR
  if (itemBuilds.boots.length > 0) {
    const bestWR = [...itemBuilds.boots].sort(
      (a, b) => b.win_rate - a.win_rate,
    )[0];
    wrBlocks.push({
      type: hideWR
        ? 'Boots'
        : `Boots :: ${bestWR.win_rate}% WR`,
      items: [makeItem(bestWR.item.id)],
    });
  }

  // Core build with highest WR
  if (itemBuilds.coreBuilds.length > 0) {
    const bestWR = [...itemBuilds.coreBuilds].sort(
      (a, b) => b.win_rate - a.win_rate,
    )[0];
    wrBlocks.push({
      type: hideWR
        ? `Core Build :: ${bestWR.play} Games`
        : `Core Build :: ${bestWR.win_rate}% WR - ${bestWR.play} Games`,
      items: bestWR.items.map((item) => makeItem(item.id)),
    });
  }

  // 4th item with highest WR
  if (itemBuilds.fourthItems.length > 0) {
    const sorted = [...itemBuilds.fourthItems].sort(
      (a, b) => b.win_rate - a.win_rate,
    );
    wrBlocks.push({
      type: '4th Item Options (by Win Rate)',
      items: sorted.map((row) => makeItem(row.item.id)),
    });
  }

  // 5th item with highest WR
  if (itemBuilds.fifthItems.length > 0) {
    const sorted = [...itemBuilds.fifthItems].sort(
      (a, b) => b.win_rate - a.win_rate,
    );
    wrBlocks.push({
      type: '5th Item Options (by Win Rate)',
      items: sorted.map((row) => makeItem(row.item.id)),
    });
  }

  // 6th item
  if (itemBuilds.sixthItems.length > 0) {
    const sorted = [...itemBuilds.sixthItems].sort(
      (a, b) => b.win_rate - a.win_rate,
    );
    wrBlocks.push({
      type: '6th Item Options (by Win Rate)',
      items: sorted.map((row) => makeItem(row.item.id)),
    });
  }

  // Only push build 2 if it has any blocks
  if (wrBlocks.length > 0) {
    builds.push({
      title: `[OP.GG] ${champion}${modeTag} - ${position || 'Build'} (Highest WR)`,
      associatedMaps: mapIds,
      associatedChampions: championId ? [championId] : [],
      blocks: wrBlocks,
      map: 'any',
      mode: 'any',
      preferredItemSlots: [],
      sortrank: 1,
      startedFrom: 'heuristic',
      type: 'custom',
    });
  }

  return builds;
}

/**
 * Transform complete OP.GG page data into a BuildSection compatible with the Rust app.
 */
export function transformPageData(data: OpggPageData): LcuBuildSection {
  const position = ''; // OP.GG /build page is the default position
  const runes = transformRunes(data.runePages, data.champion, position, data.mode);
  const itemBuilds = transformItemBuilds(
    data.itemBuilds,
    data.champion,
    position,
    undefined,
    data.mode,
  );

  const modeLabel = getModeLabel(data.mode);
  // For non-ranked modes, use the mode as the ID suffix
  const idSuffix = data.mode === 'ranked' ? 'ranked' : data.mode;

  return {
    index: 0,
    id: `opgg-${data.champion}-${idSuffix}`,
    version: data.version,
    officialVersion: data.officialVersion,
    pickCount: data.runePages[0]?.play ?? 0,
    winRate: data.runePages[0]
      ? `${(data.runePages[0].win_rate * 100).toFixed(2)}%`
      : '0%',
    timestamp: Date.now(),
    alias: data.champion,
    name: data.champion,
    position,
    skills: null,
    spells: null,
    championTier: data.championTier,
    itemBuilds,
    runes,
  };
}
