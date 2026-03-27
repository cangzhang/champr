import { PlaywrightCrawler, type PlaywrightCrawlingContext } from 'crawlee';
import type { CrawlerOptions, LcuBuildSection } from './types.js';
import { parseBuildPage } from './parser.js';
import { transformPageData } from './transform.js';
import fs from 'node:fs';
import path from 'node:path';

const BASE_URL = 'https://op.gg/lol/champions';
const DEFAULT_REGION = 'kr';
const DEFAULT_TIER = 'diamond_plus';
const DEFAULT_QUEUE_TYPE = 'ranked';
const DEFAULT_CONCURRENCY = 3;

const USER_AGENT =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';

/**
 * Build the OP.GG URL for a champion's build page.
 */
export function buildUrl(
  champion: string,
  region: string = DEFAULT_REGION,
  tier: string = DEFAULT_TIER,
  queueType: string = DEFAULT_QUEUE_TYPE,
): string {
  const params = new URLSearchParams({
    region,
    tier,
  });

  // OP.GG uses "type=flex" for flex queue, no type param for solo/duo
  if (queueType === 'flex') {
    params.set('type', 'flex');
  }

  return `${BASE_URL}/${champion}/build?${params.toString()}`;
}

/**
 * Create and run a PlaywrightCrawler for OP.GG champion builds.
 * Returns an array of BuildSection results for each champion crawled.
 */
export async function crawlChampions(
  options: CrawlerOptions,
): Promise<LcuBuildSection[]> {
  const {
    region = DEFAULT_REGION,
    tier = DEFAULT_TIER,
    queueType = DEFAULT_QUEUE_TYPE,
    outputDir = './output',
    concurrency = DEFAULT_CONCURRENCY,
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
  const errors: Array<{ champion: string; error: string }> = [];

  // Build request list
  const requests = champions.map((champion) => ({
    url: buildUrl(champion, region, tier, queueType),
    label: champion,
    userData: { champion, region, tier, queueType },
  }));

  const crawler = new PlaywrightCrawler({
    maxConcurrency: concurrency,
    requestHandlerTimeoutSecs: 60,
    navigationTimeoutSecs: 30,

    launchContext: {
      launchOptions: {
        headless: true,
        args: [
          '--disable-blink-features=AutomationControlled',
        ],
      },
    },

    browserPoolOptions: {
      useFingerprints: false,
    },

    preNavigationHooks: [
      async ({ page }) => {
        // Hide webdriver flag to avoid CloudFront bot detection
        await page.addInitScript(() => {
          Object.defineProperty(navigator, 'webdriver', { get: () => false });
        });
        // Set a realistic user agent
        await page.context().setExtraHTTPHeaders({
          'Accept-Language': 'en-US,en;q=0.9',
        });
      },
    ],

    async requestHandler(ctx: PlaywrightCrawlingContext) {
      const { page, request, log } = ctx;
      const { champion, region: r, tier: t, queueType: qt } = request.userData as {
        champion: string;
        region: string;
        tier: string;
        queueType: string;
      };

      log.info(`Crawling ${champion}...`, { url: request.url });

      try {
        // Parse the build page
        const pageData = await parseBuildPage(page, champion, r, t, qt);

        // Transform to LCU format
        const buildSection = transformPageData(pageData);

        // Write individual champion JSON
        const outputPath = path.join(outputDir, `${champion}.json`);
        fs.writeFileSync(outputPath, JSON.stringify(buildSection, null, 2));

        results.push(buildSection);
        log.info(`Successfully crawled ${champion}`, {
          runes: buildSection.runes.length,
          itemBuilds: buildSection.itemBuilds.length,
        });
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : String(err);
        log.error(`Failed to crawl ${champion}: ${errorMsg}`);
        errors.push({ champion, error: errorMsg });
        throw err; // Let crawlee handle retries
      }
    },

    failedRequestHandler({ request, log }) {
      const champion = request.userData?.champion || request.url;
      log.error(`Giving up on ${champion} after retries`);
    },
  });

  log(`Starting crawl for ${champions.length} champion(s): ${champions.join(', ')}`);

  await crawler.run(requests);

  // Write combined output
  if (results.length > 0) {
    const combinedPath = path.join(outputDir, '_all.json');
    fs.writeFileSync(combinedPath, JSON.stringify(results, null, 2));
    log(`Combined output written to ${combinedPath}`);
  }

  // Report errors
  if (errors.length > 0) {
    log(`\nErrors (${errors.length}):`);
    for (const { champion, error } of errors) {
      log(`  - ${champion}: ${error}`);
    }
  }

  log(
    `\nCrawl complete: ${results.length} succeeded, ${errors.length} failed out of ${champions.length} total`,
  );

  return results;
}

function log(msg: string) {
  console.log(`[opgg] ${msg}`);
}
