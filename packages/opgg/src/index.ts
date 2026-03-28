import { crawlChampions, fetchChampionList } from './crawler.js';
import type { CrawlerOptions, GameMode } from './types.js';

const VALID_MODES: ReadonlySet<string> = new Set(['ranked', 'aram', 'urf', 'aram-mayhem']);

/**
 * OP.GG Crawler for League of Legends champion builds.
 *
 * Usage:
 *   # Single champion (ranked by default)
 *   pnpm start leesin
 *   pnpm start leesin --region=kr --tier=diamond_plus
 *
 *   # ARAM mode
 *   pnpm start leesin --mode=aram
 *
 *   # URF mode
 *   pnpm start leesin --mode=urf
 *
 *   # ARAM Mayhem mode
 *   pnpm start leesin --mode=aram-mayhem
 *
 *   # Multiple champions
 *   pnpm start leesin,yasuo,zed --mode=aram
 *
 *   # Batch mode with a champion list file (one champion per line)
 *   pnpm start --file=champions.txt --mode=aram
 *
 *   # All champions (fetched from OP.GG)
 *   pnpm start --all --mode=aram
 */

/** Tracks whether --all was explicitly requested */
let allModeRequested = false;

function parseArgs(argv: string[]): CrawlerOptions {
  const args = argv.slice(2);
  const options: CrawlerOptions = {};

  let positional: string | undefined;

  for (const arg of args) {
    if (arg.startsWith('--region=')) {
      options.region = arg.split('=')[1];
    } else if (arg.startsWith('--tier=')) {
      options.tier = arg.split('=')[1];
    } else if (arg.startsWith('--mode=')) {
      const mode = arg.split('=')[1];
      if (!VALID_MODES.has(mode)) {
        console.error(`Invalid mode: ${mode}. Valid modes: ${[...VALID_MODES].join(', ')}`);
        process.exit(1);
      }
      options.mode = mode as GameMode;
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
      // Will fetch champion list from OP.GG
      allModeRequested = true;
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

async function main() {
  const options = parseArgs(process.argv);

  // Set defaults early so fetchChampionList can use them
  options.region ??= 'kr';
  options.tier ??= 'diamond_plus';
  options.mode ??= 'ranked';
  options.outputDir ??= './output';
  options.concurrency ??= 3;

  // Handle --all flag: fetch champion list + tiers from OP.GG
  if (allModeRequested) {
    console.log('[opgg] Fetching champion list from OP.GG...');
    const { champions, tiers } = await fetchChampionList(
      options.mode,
      options.region,
      options.tier,
    );
    options.champions = champions;
    options.championTiers = tiers;
    console.log(`[opgg] Found ${champions.length} champions with tier data`);
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
  --mode=<mode>            Game mode: ranked, aram, urf, aram-mayhem (default: ranked)
  --output=<dir>           Output directory (default: ./output)
  --concurrency=<n>        Max concurrent browsers (default: 3)

Examples:
  pnpm start leesin
  pnpm start leesin --mode=aram
  pnpm start leesin --mode=urf
  pnpm start leesin --mode=aram-mayhem
  pnpm start leesin --region=kr --tier=diamond_plus
  pnpm start leesin,yasuo,zed --mode=aram --output=./builds
  pnpm start --all --mode=aram --concurrency=5
`);
    process.exit(1);
  }

  console.log('[opgg] Configuration:', {
    champions: options.champion || options.champions,
    region: options.region,
    tier: options.tier,
    mode: options.mode,
    outputDir: options.outputDir,
    concurrency: options.concurrency,
    hasTierData: options.championTiers ? options.championTiers.size > 0 : false,
  });

  const results = await crawlChampions(options);

  // Print summary
  console.log('\n[opgg] === Results Summary ===');
  for (const section of results) {
    console.log(`\n  ${section.alias} (mode: ${options.mode}):`);
    if (section.championTier) {
      console.log(`    Champion Tier: ${section.championTier}`);
    }
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

export { crawlChampions, buildUrl, fetchChampionList } from './crawler.js';
export { parseBuildPage } from './parser.js';
export { transformPageData, transformRunes, transformItemBuilds } from './transform.js';
export type * from './types.js';
