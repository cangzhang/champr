// ============================================================
// OP.GG RSC (React Server Components) source data types
// These mirror the structure found in self.__next_f.push() payloads
// ============================================================

/** A single rune option within a row */
export interface OpggRune {
  id: number;
  name: string;
  image_url: string;
  isActive: boolean;
  play?: number;
  win?: number;
  pick_rate?: number;
  win_rate?: number;
}

/** Perk style (rune tree) info */
export interface OpggPerkStyle {
  id: number;
  name: string;
  image_url: string;
}

/** A full rune build showing all rows with active selections */
export interface OpggRuneBuild {
  id: number;
  primary_perk_style: OpggPerkStyle;
  perk_sub_style: OpggPerkStyle;
  main_runes: OpggRune[][]; // 4 rows: keystone + 3 tiers
  sub_runes: OpggRune[][];  // 3 rows from secondary tree
  shards: OpggRune[][];     // 3 rows of stat shards
}

/** Data ready for LCU import, embedded in rune_pages */
export interface OpggImportClientData {
  type: string;
  championKey: string;
  primaryStyleId: number;
  subStyleId: number;
  selectedPerkIds: number[]; // 9 IDs: 4 primary + 2 secondary + 3 shards
}

/** A rune page tab (there are 2 on the build page) */
export interface OpggRunePage {
  id: number;
  play: number;
  pick_rate: number;
  win_rate: number;
  primary_rune: { id: number; name: string };
  primary_perk_style: OpggPerkStyle;
  perk_sub_style: OpggPerkStyle;
  importClientData: OpggImportClientData;
  builds: OpggRuneBuild[];
}

/** Per-rune statistics from single_rune_builds */
export interface OpggSingleRuneStat {
  id: number;
  name: string;
  isActive: boolean;
  play?: number;
  win?: number;
  pick_rate: number;
  win_rate: number;
}

/** An item entry in a build row */
export interface OpggItemEntry {
  id: number;
  name: string;
  image_url: string;
}

/** A starter items row */
export interface OpggStarterItemRow {
  items: OpggItemEntry[];
  pick_rate: number;
  win_rate: number;
  play: number;
}

/** A boots row */
export interface OpggBootsRow {
  item: OpggItemEntry;
  pick_rate: number;
  win_rate: number;
  play: number;
}

/** A core build row (3 items) */
export interface OpggCoreBuildRow {
  items: OpggItemEntry[];
  pick_rate: number;
  win_rate: number;
  play: number;
}

/** A depth item row (4th/5th/6th item options) */
export interface OpggDepthItemRow {
  item: OpggItemEntry;
  win_rate: number;
  play: number;
}

/** Complete parsed item builds from page */
export interface OpggItemBuilds {
  starterItems: OpggStarterItemRow[];
  boots: OpggBootsRow[];
  coreBuilds: OpggCoreBuildRow[];
  fourthItems: OpggDepthItemRow[];
  fifthItems: OpggDepthItemRow[];
  sixthItems: OpggDepthItemRow[];
}

/** Supported game modes */
export type GameMode = 'ranked' | 'aram' | 'urf' | 'aram-mayhem';

/** Full parsed data from one champion's build page */
export interface OpggPageData {
  champion: string;
  region: string;
  tier: string;
  /** Game mode: ranked, aram, urf, aram-mayhem */
  mode: GameMode;
  /** Display patch version from page, e.g. "16.06" */
  version: string;
  /** Data Dragon / official version from image URLs, e.g. "16.6.1" */
  officialVersion: string;
  runePages: OpggRunePage[];
  itemBuilds: OpggItemBuilds;
  /** Champion tier from the page, e.g. "1", "2", "OP", "S" */
  championTier: string | null;
}

// ============================================================
// Output types matching Rust structs in crates/lcu/src/builds.rs
// These use camelCase as that's what serde(rename_all = "camelCase") produces
// ============================================================

/** Matches Rust `Item` struct */
export interface LcuItem {
  id: string;  // Riot item ID as string
  count: number;
}

/** Matches Rust `Block` struct */
export interface LcuBlock {
  type: string;  // display name for the section
  items: LcuItem[];
}

/** Matches Rust `ItemBuild` struct - written as JSON to Recommended/ folder */
export interface LcuItemBuild {
  title: string;
  associatedMaps: number[];
  associatedChampions: number[];
  blocks: LcuBlock[];
  map: string;
  mode: string;
  preferredItemSlots: string[];
  sortrank: number;
  startedFrom: string;
  type: string;
}

/** Matches Rust `Rune` struct - sent to LCU API POST /lol-perks/v1/pages */
export interface LcuRune {
  uuid: string;
  alias: string;
  name: string;
  position: string;
  pickCount: number;
  winRate: string;
  primaryStyleId: number;
  subStyleId: number;
  selectedPerkIds: number[];
  score: number | null;
  type: string;
}

/** Matches Rust `BuildSection` struct */
export interface LcuBuildSection {
  index: number;
  id: string;
  version: string;
  officialVersion: string;
  pickCount: number;
  winRate: string;
  timestamp: number;
  alias: string;
  name: string;
  position: string;
  skills: string[] | null;
  spells: string[] | null;
  championTier: string | null;
  itemBuilds: LcuItemBuild[];
  runes: LcuRune[];
}

/** Crawler configuration options */
export interface CrawlerOptions {
  champion?: string;       // single champion alias (e.g., "leesin")
  champions?: string[];    // batch mode: list of champion aliases
  region?: string;         // default: "kr"
  tier?: string;           // default: "diamond_plus"
  mode?: GameMode;         // default: "ranked" (also: aram, urf, aram-mayhem)
  outputDir?: string;      // default: "./output"
  concurrency?: number;    // default: 3
  position?: string;       // default: "" (all positions from page)
  championTiers?: Map<string, number>; // pre-fetched tier map from OP.GG champion list
}

// ============================================================
// Crawl verification / report types
// ============================================================

/** Verification status for a single champion crawl */
export type CrawlStatusValue = 'success' | 'partial' | 'failed';

/**
 * Per-champion crawl result entry included in the final report.
 *
 * - status 'success'  : runes AND item builds were both found
 * - status 'partial'  : only one of runes / item builds was found
 * - status 'failed'   : champion was never successfully crawled (all retries exhausted)
 */
export interface ChampionCrawlStatus {
  /** Champion alias, e.g. "leesin" */
  champion: string;
  /** Game mode this entry belongs to */
  mode: GameMode;
  /** Crawl outcome */
  status: CrawlStatusValue;
  /** Human-readable explanation for 'partial' or 'failed' entries */
  reason?: string;
  /** Relative path to the written JSON file (only set for 'success' / 'partial') */
  outputFile?: string;
  /** Number of rune pages in the output (0 for 'failed') */
  runes: number;
  /** Number of item build sets in the output (0 for 'failed') */
  itemBuilds: number;
  /** Champion tier string if available, e.g. "1", "2" */
  championTier?: string | null;
  /** ISO-8601 timestamp of when this entry was recorded */
  timestamp: string;
}

/** Final crawl report written to crawl-report[-{mode}].json */
export interface CrawlReport {
  /** ISO-8601 timestamp of when the report was generated */
  generatedAt: string;
  /** Game mode that was crawled */
  mode: GameMode;
  /** Region used during crawl */
  region: string;
  /** Tier filter used during crawl */
  tier: string;
  /** Total number of champions attempted */
  total: number;
  /** Champions with status 'success' */
  succeeded: number;
  /** Champions with status 'partial' */
  partial: number;
  /** Champions with status 'failed' */
  failed: number;
  /** Per-champion details, sorted alphabetically by champion alias */
  champions: ChampionCrawlStatus[];
}
