import type { Page } from 'playwright';
import type {
  OpggRunePage,
  OpggItemBuilds,
  OpggStarterItemRow,
  OpggBootsRow,
  OpggCoreBuildRow,
  OpggDepthItemRow,
  OpggPageData,
  GameMode,
} from './types.js';

/**
 * Extract all RSC flight data chunks from the page's self.__next_f.push() calls.
 * Returns the concatenated string payload.
 */
async function extractRscChunks(page: Page): Promise<string[]> {
  return page.evaluate(`
    (() => {
      const chunks = [];
      const scripts = document.querySelectorAll('script');
      for (const script of scripts) {
        const text = script.textContent || '';
        const match = text.match(/self\\.__next_f\\.push\\(\\[1,"(.*)"\\]\\)/s);
        if (match) {
          const raw = match[1]
            .replace(/\\\\u([0-9a-fA-F]{4})/g, (_, hex) =>
              String.fromCharCode(parseInt(hex, 16))
            )
            .replace(/\\\\n/g, '\\n')
            .replace(/\\\\"/g, '"')
            .replace(/\\\\\\\\/g, '\\\\');
          chunks.push(raw);
        }
      }
      return chunks;
    })()
  `);
}

/**
 * Find and parse the rune_pages JSON from RSC chunks.
 * The data is passed as a `data` prop to a client component.
 */
function parseRunePages(chunks: string[]): OpggRunePage[] {
  for (const chunk of chunks) {
    const idx = chunk.indexOf('"rune_pages"');
    if (idx === -1) continue;

    // Find the start of the containing object
    let braceStart = chunk.lastIndexOf('{', idx);
    if (braceStart === -1) continue;

    // Extract the balanced JSON object
    const jsonStr = extractBalancedJson(chunk, braceStart);
    if (!jsonStr) continue;

    try {
      const data = JSON.parse(jsonStr);
      if (data.rune_pages && Array.isArray(data.rune_pages)) {
        return data.rune_pages as OpggRunePage[];
      }
    } catch {
      continue;
    }
  }

  throw new Error('Failed to parse rune_pages from RSC data (no rune_pages found in any chunk)');
}

/**
 * Try to parse rune pages, returning empty array if not found.
 * Some modes (ARAM Mayhem) may not have rune data.
 */
function tryParseRunePages(chunks: string[]): OpggRunePage[] {
  try {
    return parseRunePages(chunks);
  } catch {
    return [];
  }
}

/**
 * Extract a balanced JSON object/array starting at the given position.
 */
function extractBalancedJson(str: string, start: number): string | null {
  const openChar = str[start];
  const closeChar = openChar === '{' ? '}' : openChar === '[' ? ']' : null;
  if (!closeChar) return null;

  let depth = 0;
  let inString = false;
  let escaped = false;

  for (let i = start; i < str.length; i++) {
    const ch = str[i];

    if (escaped) {
      escaped = false;
      continue;
    }

    if (ch === '\\' && inString) {
      escaped = true;
      continue;
    }

    if (ch === '"') {
      inString = !inString;
      continue;
    }

    if (inString) continue;

    if (ch === openChar) {
      depth++;
    } else if (ch === closeChar) {
      depth--;
      if (depth === 0) {
        return str.substring(start, i + 1);
      }
    }
  }

  return null;
}

// We use a string-based evaluate to avoid tsx/esbuild injecting __name helpers
// into the browser context. All DOM scraping logic is in this string.
const ITEM_SCRAPING_SCRIPT = `
(() => {
  function parseNum(text) {
    return parseFloat(text.replace(/,/g, '').replace('%', '').trim());
  }

  function extractItemIds(row) {
    const ids = [];
    const imgs = row.querySelectorAll('img[src*="/item/"]');
    for (const img of imgs) {
      const src = img.getAttribute('src') || '';
      const match = src.match(/\\/item\\/(\\d+)\\./);
      if (match) {
        ids.push(parseInt(match[1], 10));
      }
    }
    return ids;
  }

  function extractItemNames(row) {
    const names = [];
    const imgs = row.querySelectorAll('img[src*="/item/"]');
    for (const img of imgs) {
      names.push(img.getAttribute('alt') || '');
    }
    return names;
  }

  function findTableByCaption(text) {
    const captions = document.querySelectorAll('caption');
    for (const caption of captions) {
      if (caption.textContent && caption.textContent.includes(text)) {
        return caption.closest('table');
      }
    }
    return null;
  }

  function parseTableRows(table) {
    const rows = [];
    const trs = table.querySelectorAll('tbody tr');
    for (const tr of trs) {
      const itemIds = extractItemIds(tr);
      if (itemIds.length === 0) continue;
      const itemNames = extractItemNames(tr);

      const strongs = tr.querySelectorAll('td strong');
      const values = [];
      for (const strong of strongs) {
        const text = strong.textContent || '';
        if (text.includes('%')) {
          values.push(parseNum(text));
        }
      }

      const spans = tr.querySelectorAll('td span');
      let gameCount = 0;
      for (const span of spans) {
        const text = span.textContent || '';
        if (text.includes('Game')) {
          const numMatch = text.match(/([\\d,]+)/);
          if (numMatch) {
            gameCount = parseNum(numMatch[1]);
          }
        }
      }

      rows.push({ itemIds, itemNames, values, gameCount });
    }
    return rows;
  }

  // Parse starter items (caption is "Items Table")
  const starterItems = [];
  const starterTable = findTableByCaption('Items Table');
  if (starterTable) {
    for (const row of parseTableRows(starterTable)) {
      starterItems.push({
        items: row.itemIds.map((id, i) => ({
          id: id,
          name: row.itemNames[i] || '',
          image_url: '',
        })),
        pick_rate: row.values[0] || 0,
        win_rate: row.values[1] || 0,
        play: row.gameCount,
      });
    }
  }

  // Parse boots (caption is "Boots Table")
  const boots = [];
  const bootsTable = findTableByCaption('Boots Table');
  if (bootsTable) {
    for (const row of parseTableRows(bootsTable)) {
      boots.push({
        item: {
          id: row.itemIds[0],
          name: row.itemNames[0] || '',
          image_url: '',
        },
        pick_rate: row.values[0] || 0,
        win_rate: row.values[1] || 0,
        play: row.gameCount,
      });
    }
  }

  // Parse core builds (caption is "Builds Table")
  const coreBuilds = [];
  const coreTable = findTableByCaption('Builds Table');
  if (coreTable) {
    for (const row of parseTableRows(coreTable)) {
      coreBuilds.push({
        items: row.itemIds.map((id, i) => ({
          id: id,
          name: row.itemNames[i] || '',
          image_url: '',
        })),
        pick_rate: row.values[0] || 0,
        win_rate: row.values[1] || 0,
        play: row.gameCount,
      });
    }
  }

  // Parse depth items (4th, 5th, 6th)
  function parseDepthItems(captionText) {
    const items = [];
    const table = findTableByCaption(captionText);
    if (table) {
      for (const row of parseTableRows(table)) {
        items.push({
          item: {
            id: row.itemIds[0],
            name: row.itemNames[0] || '',
            image_url: '',
          },
          win_rate: row.values[0] || 0,
          play: row.gameCount,
        });
      }
    }
    return items;
  }

  const fourthItems = parseDepthItems('Depth 4 Items Table');
  const fifthItems = parseDepthItems('Depth 5 Items Table');
  const sixthItems = parseDepthItems('Depth 6 Items Table');

  return {
    starterItems,
    boots,
    coreBuilds,
    fourthItems,
    fifthItems,
    sixthItems,
  };
})()
`;

/**
 * Parse item builds by scraping the rendered DOM.
 * Uses a string-based evaluate to avoid tsx __name helper injection issues.
 */
async function parseItemBuilds(page: Page): Promise<OpggItemBuilds> {
  return page.evaluate(ITEM_SCRAPING_SCRIPT) as Promise<OpggItemBuilds>;
}

/**
 * Extract patch version info from the page.
 * Returns { version, officialVersion } e.g. { version: "16.06", officialVersion: "16.6.1" }
 */
async function parseVersion(page: Page): Promise<{ version: string; officialVersion: string }> {
  return page.evaluate(`
    (() => {
      var version = '';
      var officialVersion = '';

      // Extract display patch from page text, e.g. "Patch 16.06"
      var body = document.body.innerText || '';
      var patchMatch = body.match(/Patch\\s+(\\d+\\.\\d+)/);
      if (patchMatch) {
        version = patchMatch[1];
      }

      // Extract Data Dragon version from image URLs, e.g. "/16.6.1/"
      var imgs = document.querySelectorAll('img');
      for (var img of imgs) {
        var src = img.getAttribute('src') || '';
        var imgMatch = src.match(/\\/(\\d+\\.\\d+\\.\\d+)\\//);
        if (imgMatch) {
          officialVersion = imgMatch[1];
          break;
        }
      }

      return { version: version, officialVersion: officialVersion };
    })()
  `) as Promise<{ version: string; officialVersion: string }>;
}

/** Champion info extracted from OP.GG champion list in RSC data */
export interface OpggChampionInfo {
  key: string;    // URL slug, e.g. "leesin"
  name: string;   // Display name, e.g. "Lee Sin"
  tier: number;   // Tier integer 1-5
}

/**
 * Extract the full champion list from a mode page's RSC data.
 * Mode pages (ARAM, URF, etc.) have a sidebar component with a "champions":[...] array
 * containing every champion with key, name, tier, etc.
 */
export async function extractModeChampionList(page: Page): Promise<OpggChampionInfo[]> {
  const chunks = await extractRscChunks(page);

  for (const chunk of chunks) {
    // Look for the champions array in RSC props
    const idx = chunk.indexOf('"champions":[');
    if (idx === -1) continue;

    // Find the start of the array
    const arrayStart = chunk.indexOf('[', idx);
    if (arrayStart === -1) continue;

    const jsonStr = extractBalancedJson(chunk, arrayStart);
    if (!jsonStr) continue;

    try {
      const champions = JSON.parse(jsonStr) as Array<{
        key?: string;
        name?: string;
        tier?: number;
        id?: number;
      }>;

      if (!Array.isArray(champions) || champions.length === 0) continue;
      // Validate it looks like champion data
      if (!champions[0].key || champions[0].tier === undefined) continue;

      return champions
        .filter((c) => c.key && c.name && typeof c.tier === 'number')
        .map((c) => ({
          key: c.key!,
          name: c.name!,
          tier: c.tier!,
        }));
    } catch {
      continue;
    }
  }

  throw new Error('Failed to extract champion list from mode page RSC data');
}

/**
 * Extract the full champion list from the ranked tier list page's RSC data.
 * The ranked tier list page (/lol/champions?region=...&tier=...) has a "data":[...] array
 * where each entry has key, name, positionTier, etc. Same champion may appear multiple times
 * for different positions — we deduplicate and take the best (lowest) tier.
 */
export async function extractRankedChampionList(page: Page): Promise<OpggChampionInfo[]> {
  const chunks = await extractRscChunks(page);

  for (const chunk of chunks) {
    // Look for "data":[ with positionName fields (distinguishes from other "data" arrays)
    const idx = chunk.indexOf('"positionName"');
    if (idx === -1) continue;

    // Search backwards from positionName to find the containing "data":[ array
    // Walk back to find "data":[
    let searchStart = idx;
    let dataIdx = -1;
    while (searchStart > 0) {
      dataIdx = chunk.lastIndexOf('"data":[', searchStart);
      if (dataIdx !== -1) break;
      searchStart -= 1000;
    }
    if (dataIdx === -1) continue;

    const arrayStart = chunk.indexOf('[', dataIdx);
    if (arrayStart === -1) continue;

    const jsonStr = extractBalancedJson(chunk, arrayStart);
    if (!jsonStr) continue;

    try {
      const data = JSON.parse(jsonStr) as Array<{
        key?: string;
        name?: string;
        positionName?: string;
        positionTierData?: { tier?: number; rank?: number };
      }>;

      if (!Array.isArray(data) || data.length === 0) continue;
      if (!data[0].key || !data[0].positionName) continue;

      // Deduplicate by champion key, keeping the best (lowest) tier
      const tierMap = new Map<string, { name: string; tier: number }>();

      for (const entry of data) {
        if (!entry.key || !entry.name) continue;
        const tier = entry.positionTierData?.tier ?? 5;
        const existing = tierMap.get(entry.key);
        if (!existing || tier < existing.tier) {
          tierMap.set(entry.key, { name: entry.name, tier });
        }
      }

      return Array.from(tierMap.entries()).map(([key, { name, tier }]) => ({
        key,
        name,
        tier,
      }));
    } catch {
      continue;
    }
  }

  throw new Error('Failed to extract champion list from ranked tier list page RSC data');
}

/**
 * Main parser: extract all build data from an OP.GG champion build page.
 * Supports ranked and alternative game modes (ARAM, URF, ARAM Mayhem).
 * Note: championTier is set to null here — caller should override from pre-fetched tier map.
 */
export async function parseBuildPage(
  page: Page,
  champion: string,
  region: string,
  tier: string,
  mode: GameMode = 'ranked',
): Promise<OpggPageData> {
  // Wait for page content to load.
  // Some modes (ARAM Mayhem) may have sparse data, so we use a more lenient approach.
  const hasPerks = await page.waitForSelector('img[src*="/perk/"]', { timeout: 15000 }).then(() => true).catch(() => false);
  const hasItems = await page.waitForSelector('img[src*="/item/"]', { timeout: 10000 }).then(() => true).catch(() => false);

  if (!hasPerks && !hasItems) {
    // If neither runes nor items loaded, wait a bit for any content
    await page.waitForTimeout(3000);
  }

  // Extract RSC chunks for rune data
  const chunks = await extractRscChunks(page);

  // For some modes, rune data may be absent - use graceful parsing
  const runePages = tryParseRunePages(chunks);

  // Extract item builds from DOM (returns empty categories if tables are missing)
  const itemBuilds = await parseItemBuilds(page);

  // Extract patch version
  const { version, officialVersion } = await parseVersion(page);

  return {
    champion,
    region,
    tier,
    mode,
    version,
    officialVersion,
    runePages,
    itemBuilds,
    championTier: null, // Set by caller from pre-fetched tier map
  };
}

export { extractRscChunks, parseRunePages, parseItemBuilds, extractBalancedJson };
