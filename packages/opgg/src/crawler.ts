import { PlaywrightCrawler, type PlaywrightCrawlingContext } from 'crawlee';
import type { CrawlerOptions, GameMode, LcuBuildSection } from './types.js';
import { parseBuildPage, extractModeChampionList, extractRankedChampionList, type OpggChampionInfo } from './parser.js';
import { transformPageData } from './transform.js';
import fs from 'node:fs';
import path from 'node:path';

const BASE_URL = 'https://op.gg/lol/champions';
const MODES_BASE_URL = 'https://op.gg/lol/modes';
const DEFAULT_REGION = 'kr';
const DEFAULT_TIER = 'diamond_plus';
const DEFAULT_MODE: GameMode = 'ranked';
const DEFAULT_CONCURRENCY = 3;

const RETRY_DELAY_MS = 30_000;

const USER_AGENT =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36';

/** Modes that use the /lol/modes/{mode}/ URL pattern instead of /lol/champions/ */
const ALT_MODES: ReadonlySet<GameMode> = new Set(['aram', 'urf', 'aram-mayhem']);

/** Shared anti-bot pre-navigation hooks */
const ANTI_BOT_HOOKS = [
  async ({ page }: { page: import('playwright').Page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'webdriver', { get: () => false });
      Object.defineProperty(navigator, 'languages', { get: () => ['en-US', 'en'] });
      Object.defineProperty(navigator, 'plugins', { get: () => [1, 2, 3, 4, 5] });
      // @ts-ignore
      window.chrome = { runtime: {} };
    });
    await page.context().setExtraHTTPHeaders({
      'Accept-Language': 'en-US,en;q=0.9',
      'User-Agent': USER_AGENT,
      'sec-ch-ua': '"Chromium";v="134", "Google Chrome";v="134", "Not:A-Brand";v="24"',
      'sec-ch-ua-mobile': '?0',
      'sec-ch-ua-platform': '"Windows"',
    });
  },
];

/** Shared launch options */
const LAUNCH_OPTIONS = {
  headless: true,
  args: [
    '--disable-blink-features=AutomationControlled',
    '--no-sandbox',
    '--disable-setuid-sandbox',
    '--disable-dev-shm-usage',
    '--disable-gpu',
    `--user-agent=${USER_AGENT}`,
  ],
};

/**
 * Build the OP.GG URL for a champion's build page.
 * Ranked:     /lol/champions/{champ}/build?region=...&tier=...
 * Alt modes:  /lol/modes/{mode}/{champ}/build
 */
export function buildUrl(
  champion: string,
  region: string = DEFAULT_REGION,
  tier: string = DEFAULT_TIER,
  mode: GameMode = DEFAULT_MODE,
): string {
  if (ALT_MODES.has(mode)) {
    return `${MODES_BASE_URL}/${mode}/${champion}/build`;
  }

  const params = new URLSearchParams({ region, tier });
  return `${BASE_URL}/${champion}/build?${params.toString()}`;
}

/**
 * Create a mini PlaywrightCrawler to pre-fetch the champion list + tiers from OP.GG.
 *
 * For mode pages (ARAM/URF/ARAM Mayhem): opens one build page and extracts the
 * sidebar's "champions" array from RSC data.
 *
 * For ranked: opens the tier list page (/lol/champions?region=...&tier=...) and
 * extracts the "data" array.
 *
 * Returns { champions: string[], tiers: Map<string, number> }
 */
export async function fetchChampionList(
  mode: GameMode = DEFAULT_MODE,
  region: string = DEFAULT_REGION,
  tier: string = DEFAULT_TIER,
): Promise<{ champions: string[]; tiers: Map<string, number> }> {
  let url: string;

  if (ALT_MODES.has(mode)) {
    url = `${MODES_BASE_URL}/${mode}/aatrox/build`;
  } else {
    url = `${BASE_URL}?region=${region}&tier=${tier}`;
  }

  let result: OpggChampionInfo[] = [];

  const crawler = new PlaywrightCrawler({
    maxConcurrency: 1,
    requestHandlerTimeoutSecs: 90,
    navigationTimeoutSecs: 60,
    maxRequestRetries: 3,
    launchContext: { launchOptions: LAUNCH_OPTIONS },
    browserPoolOptions: { useFingerprints: false },
    preNavigationHooks: ANTI_BOT_HOOKS,

    async requestHandler(ctx: PlaywrightCrawlingContext) {
      const { page, log } = ctx;

      await page.waitForSelector('img', { timeout: 30000 }).catch(() => {});
      await page.waitForTimeout(3000);

      if (ALT_MODES.has(mode)) {
        log.info(`Extracting champion list from mode page (${mode})...`);
        result = await extractModeChampionList(page);
      } else {
        log.info('Extracting champion list from ranked tier list page...');
        result = await extractRankedChampionList(page);
      }

      log.info(`Found ${result.length} champions`);
    },
  });

  log(`Fetching champion list from OP.GG (mode: ${mode})...`);
  await crawler.run([{ url, label: 'champion-list' }]);

  if (result.length === 0) {
    throw new Error('Failed to fetch champion list from OP.GG — got 0 champions');
  }

  const champions = result.map((c) => c.key);
  const tiers = new Map<string, number>();
  for (const c of result) {
    tiers.set(c.key, c.tier);
  }

  log(`Champion list: ${champions.length} champions, all with tiers`);

  return { champions, tiers };
}

/**
 * Create the request handler for champion build crawling.
 * Shared between main crawl and retry pass.
 */
function makeRequestHandler(
  results: LcuBuildSection[],
  outputDir: string,
  championTiers: Map<string, number> | undefined,
) {
  return async (ctx: PlaywrightCrawlingContext) => {
    const { page, request, log } = ctx;
    const { champion, region: r, tier: t, mode: m } = request.userData as {
      champion: string;
      region: string;
      tier: string;
      mode: GameMode;
    };

    log.info(`Crawling ${champion} (mode: ${m})...`, { url: request.url });

    const pageData = await parseBuildPage(page, champion, r, t, m);
    const buildSection = transformPageData(pageData);

    // Override championTier from pre-fetched tier map if available
    if (championTiers && championTiers.has(champion)) {
      buildSection.championTier = String(championTiers.get(champion));
    }

    // Write individual champion JSON with mode suffix
    const fileName = m === 'ranked' ? `${champion}.json` : `${champion}-${m}.json`;
    const outputPath = path.join(outputDir, fileName);
    fs.writeFileSync(outputPath, JSON.stringify(buildSection, null, 2));

    results.push(buildSection);
    log.info(`Successfully crawled ${champion} (${m})`, {
      runes: buildSection.runes.length,
      itemBuilds: buildSection.itemBuilds.length,
      championTier: buildSection.championTier,
    });
  };
}

/**
 * Create and run a PlaywrightCrawler for OP.GG champion builds.
 *
 * After the main crawl, any champions that failed all retries are queued for
 * a single retry pass (30s cooldown, concurrency 1, longer timeouts) to recover
 * from transient bot-detection or rate-limiting issues.
 *
 * Returns an array of BuildSection results for each champion crawled.
 */
export async function crawlChampions(
  options: CrawlerOptions,
): Promise<LcuBuildSection[]> {
  const {
    region = DEFAULT_REGION,
    tier = DEFAULT_TIER,
    mode = DEFAULT_MODE,
    outputDir = './output',
    concurrency = DEFAULT_CONCURRENCY,
    championTiers,
  } = options;

  // Build the list of champions to crawl
  let champions: string[] = [];
  if (options.champion) {
    champions = [options.champion];
  } else if (options.champions && options.champions.length > 0) {
    champions = options.champions;
  } else {
    throw new Error('Either champion or champions must be specified');
  }

  // Ensure output directory exists
  fs.mkdirSync(outputDir, { recursive: true });

  const results: LcuBuildSection[] = [];
  const failedChampions: string[] = [];

  // Build request list
  const requests = champions.map((champion) => ({
    url: buildUrl(champion, region, tier, mode),
    label: champion,
    userData: { champion, region, tier, mode },
  }));

  // ── Main crawl pass ──────────────────────────────────────────

  const crawler = new PlaywrightCrawler({
    maxConcurrency: concurrency,
    requestHandlerTimeoutSecs: 60,
    navigationTimeoutSecs: 30,
    launchContext: { launchOptions: LAUNCH_OPTIONS },
    browserPoolOptions: { useFingerprints: false },
    preNavigationHooks: ANTI_BOT_HOOKS,
    requestHandler: makeRequestHandler(results, outputDir, championTiers),

    failedRequestHandler({ request, log }) {
      const champion = (request.userData?.champion as string) || request.url;
      log.error(`Giving up on ${champion} after retries`);
      failedChampions.push(champion);
    },
  });

  log(`Starting crawl for ${champions.length} champion(s) in mode "${mode}": ${champions.join(', ')}`);
  await crawler.run(requests);

  // ── Retry pass for failed champions ──────────────────────────

  let retryRecovered = 0;
  const finalFailed: string[] = [];

  if (failedChampions.length > 0) {
    log('');
    log(`${failedChampions.length} champion(s) failed. Retrying in ${RETRY_DELAY_MS / 1000}s with concurrency 1...`);
    log(`  Retry queue: [${failedChampions.join(', ')}]`);

    await new Promise((r) => setTimeout(r, RETRY_DELAY_MS));

    const retryRequests = failedChampions.map((champion) => ({
      url: buildUrl(champion, region, tier, mode),
      label: `retry-${champion}`,
      userData: { champion, region, tier, mode },
    }));

    const retryCrawler = new PlaywrightCrawler({
      maxConcurrency: 1,
      requestHandlerTimeoutSecs: 90,
      navigationTimeoutSecs: 45,
      launchContext: { launchOptions: LAUNCH_OPTIONS },
      browserPoolOptions: { useFingerprints: false },
      preNavigationHooks: ANTI_BOT_HOOKS,
      requestHandler: makeRequestHandler(results, outputDir, championTiers),

      failedRequestHandler({ request, log }) {
        const champion = (request.userData?.champion as string) || request.url;
        log.error(`Retry failed for ${champion}`);
        finalFailed.push(champion);
      },
    });

    const resultsBefore = results.length;
    await retryCrawler.run(retryRequests);
    retryRecovered = results.length - resultsBefore;

    if (retryRecovered > 0) {
      log(`Retry recovered ${retryRecovered} champion(s)`);
    }
  }

  // ── Write combined output ────────────────────────────────────

  if (results.length > 0) {
    const combinedName = mode === 'ranked' ? '_all.json' : `_all-${mode}.json`;
    const combinedPath = path.join(outputDir, combinedName);
    fs.writeFileSync(combinedPath, JSON.stringify(results, null, 2));
    log(`Combined output written to ${combinedPath}`);
  }

  // ── Crawl Summary ────────────────────────────────────────────

  const totalFailed = finalFailed.length;
  const succeededStr = retryRecovered > 0
    ? `${results.length} (+ ${retryRecovered} recovered on retry)`
    : `${results.length}`;

  log('');
  log('='.repeat(50));
  log('  Crawl Summary');
  log('='.repeat(50));
  log(`  Mode:      ${mode}`);
  log(`  Total:     ${champions.length}`);
  log(`  Succeeded: ${succeededStr}`);
  log(`  Failed:    ${totalFailed}`);
  if (totalFailed > 0) {
    log(`  Failed:    [${finalFailed.join(', ')}]`);
  }
  log('='.repeat(50));

  return results;
}

function log(msg: string) {
  console.log(`[opgg] ${msg}`);
}
