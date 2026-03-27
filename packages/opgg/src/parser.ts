import type { Page } from 'playwright';
import type {
  OpggRunePage,
  OpggItemBuilds,
  OpggStarterItemRow,
  OpggBootsRow,
  OpggCoreBuildRow,
  OpggDepthItemRow,
  OpggPageData,
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

  throw new Error('Failed to parse rune_pages from RSC data');
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

/**
 * Main parser: extract all build data from an OP.GG champion build page.
 */
export async function parseBuildPage(
  page: Page,
  champion: string,
  region: string,
  tier: string,
  queueType: string,
): Promise<OpggPageData> {
  // Wait for the rune section to be visible (indicates page is loaded)
  await page.waitForSelector('img[src*="/perk/"]', { timeout: 15000 });
  // Also wait for item tables
  await page.waitForSelector('img[src*="/item/"]', { timeout: 15000 });

  // Extract RSC chunks for rune data
  const chunks = await extractRscChunks(page);
  const runePages = parseRunePages(chunks);

  // Extract item builds from DOM
  const itemBuilds = await parseItemBuilds(page);

  // Extract patch version
  const { version, officialVersion } = await parseVersion(page);

  return {
    champion,
    region,
    tier,
    queueType,
    version,
    officialVersion,
    runePages,
    itemBuilds,
  };
}

export { extractRscChunks, parseRunePages, parseItemBuilds };
