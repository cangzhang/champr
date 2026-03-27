import { crawlChampions } from './crawler.js';
import type { CrawlerOptions } from './types.js';

/**
 * OP.GG Crawler for League of Legends champion builds.
 *
 * Usage:
 *   # Single champion
 *   pnpm start leesin
 *   pnpm start leesin --region=kr --tier=diamond_plus --type=flex
 *
 *   # Multiple champions
 *   pnpm start leesin,yasuo,zed
 *
 *   # Batch mode with a champion list file (one champion per line)
 *   pnpm start --file=champions.txt
 *
 *   # All champions from Data Dragon
 *   pnpm start --all --region=kr
 */

function parseArgs(argv: string[]): CrawlerOptions {
  const args = argv.slice(2);
  const options: CrawlerOptions = {};

  let positional: string | undefined;

  for (const arg of args) {
    if (arg.startsWith('--region=')) {
      options.region = arg.split('=')[1];
    } else if (arg.startsWith('--tier=')) {
      options.tier = arg.split('=')[1];
    } else if (arg.startsWith('--type=')) {
      options.queueType = arg.split('=')[1];
    } else if (arg.startsWith('--output=')) {
      options.outputDir = arg.split('=')[1];
    } else if (arg.startsWith('--concurrency=')) {
      options.concurrency = parseInt(arg.split('=')[1], 10);
    } else if (arg.startsWith('--file=')) {
      const filePath = arg.split('=')[1];
      const fs = require('node:fs');
      const content = fs.readFileSync(filePath, 'utf-8');
      options.champions = content
        .split('\n')
        .map((line: string) => line.trim())
        .filter((line: string) => line && !line.startsWith('#'));
    } else if (arg === '--all') {
      // Fetch all champion names from Data Dragon
      options.champions = []; // Will be populated below
    } else if (arg.startsWith('--')) {
      console.error(`Unknown option: ${arg}`);
      process.exit(1);
    } else {
      positional = arg;
    }
  }

  // Parse positional arg as champion name(s)
  if (positional) {
    if (positional.includes(',')) {
      options.champions = positional.split(',').map((c) => c.trim().toLowerCase());
    } else {
      options.champion = positional.toLowerCase();
    }
  }

  return options;
}

async function fetchAllChampions(): Promise<string[]> {
  const url =
    'https://ddragon.leagueoflegends.com/cdn/15.6.1/data/en_US/champion.json';
  const resp = await fetch(url);
  const data = (await resp.json()) as {
    data: Record<string, { id: string }>;
  };
  return Object.values(data.data).map((c) => c.id.toLowerCase());
}

async function main() {
  const options = parseArgs(process.argv);

  // Handle --all flag
  if (options.champions && options.champions.length === 0 && !options.champion) {
    console.log('[opgg] Fetching champion list from Data Dragon...');
    options.champions = await fetchAllChampions();
    console.log(`[opgg] Found ${options.champions.length} champions`);
  }

  // Validate we have something to crawl
  if (!options.champion && (!options.champions || options.champions.length === 0)) {
    console.error(`
OP.GG Champion Build Crawler

Usage:
  pnpm start <champion>                    Crawl a single champion
  pnpm start <champ1>,<champ2>,<champ3>    Crawl multiple champions
  pnpm start --file=champions.txt          Crawl from a file list
  pnpm start --all                         Crawl all champions

Options:
  --region=<region>        Region (default: kr)
  --tier=<tier>            Tier (default: diamond_plus)
  --type=<type>            Queue type: ranked, flex (default: ranked)
  --output=<dir>           Output directory (default: ./output)
  --concurrency=<n>        Max concurrent browsers (default: 3)

Examples:
  pnpm start leesin
  pnpm start leesin --region=kr --tier=diamond_plus --type=flex
  pnpm start leesin,yasuo,zed --output=./builds
  pnpm start --all --concurrency=5
`);
    process.exit(1);
  }

  // Set defaults
  options.region ??= 'kr';
  options.tier ??= 'diamond_plus';
  options.queueType ??= 'ranked';
  options.outputDir ??= './output';
  options.concurrency ??= 3;

  console.log('[opgg] Configuration:', {
    champions: options.champion || options.champions,
    region: options.region,
    tier: options.tier,
    queueType: options.queueType,
    outputDir: options.outputDir,
    concurrency: options.concurrency,
  });

  const results = await crawlChampions(options);

  // Print summary
  console.log('\n[opgg] === Results Summary ===');
  for (const section of results) {
    console.log(`\n  ${section.alias}:`);
    console.log(`    Runes: ${section.runes.length} page(s)`);
    for (const rune of section.runes) {
      console.log(
        `      - ${rune.name} | ${rune.primaryStyleId}/${rune.subStyleId} | Perks: [${rune.selectedPerkIds.join(', ')}]`,
      );
    }
    console.log(`    Item Builds: ${section.itemBuilds.length} set(s)`);
    for (const build of section.itemBuilds) {
      console.log(`      - ${build.title} (${build.blocks.length} blocks)`);
    }
  }

  console.log(`\n[opgg] Output written to: ${options.outputDir}`);
}

main().catch((err) => {
  console.error('[opgg] Fatal error:', err);
  process.exit(1);
});

export { crawlChampions, buildUrl } from './crawler.js';
export { parseBuildPage } from './parser.js';
export { transformPageData, transformRunes, transformItemBuilds } from './transform.js';
export type * from './types.js';
