//! `Minions.lua`, `Spectres.lua`, `Bosses.lua`, `BossSkills.lua`,
//! `WorldAreas.lua`, `Misc.lua`, `CurrencyNames.lua`,
//! `CharacterMeleeSkills.lua` and `Costs.lua`, ported from PoB's
//! `minions.lua`, `spectreList.lua`, `bossData.lua`, `worldAreas.lua`,
//! `miscdata.lua` and `costs.lua`.

use crate::dat::relational::{LoadedTable, Row};
use crate::data_export::Ctx;
use crate::pob_export::statdesc::lua_literal;
use crate::pob_export::text::round;
use crate::pob_export::{game, read_text, write, Lua, Table};
use crate::settings::Game;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// PoB's names for the PoE 2 player minions it models, and the limit each
/// counts against (`Export/Minions/Minions.txt`). Ids are under
/// `Metadata/Monsters/`.
const POE2_MINIONS: [(&str, &str, Option<&str>); 32] = [
    ("Zombies/PlayerSummoned/PlayerSummonedZombie_", "RaisedZombie", Some("ActiveZombieLimit")),
    ("RagingSpirit/RagingSpiritPlayerSummoned", "SummonedRagingSpirit", Some("ActiveRagingSpiritLimit")),
    ("Skeletons/PlayerSummoned/SkeletonArcherPlayerSummoned", "RaisedSkeletonSniper", Some("ActiveSkeletonLimit")),
    ("Skeletons/PlayerSummoned/SkeletonBrutePlayerSummoned_", "RaisedSkeletonBrute", Some("ActiveSkeletonLimit")),
    ("Skeletons/PlayerSummoned/SkeletonStormMagePlayerSummoned_", "RaisedSkeletonStormMage", Some("ActiveSkeletonLimit")),
    ("Skeletons/PlayerSummoned/SkeletonFrostMagePlayerSummoned", "RaisedSkeletonFrostMage", Some("ActiveSkeletonLimit")),
    ("Skeletons/PlayerSummoned/SkeletonClericPlayerSummoned_", "RaisedSkeletonCleric", Some("ActiveSkeletonLimit")),
    ("Skeletons/PlayerSummoned/SkeletonArsonistPlayerSummoned", "RaisedSkeletonArsonist", Some("ActiveSkeletonLimit")),
    ("Skeletons/PlayerSummoned/SkeletonReaverPlayerSummoned", "RaisedSkeletonReaver", Some("ActiveSkeletonLimit")),
    ("Skeletons/PlayerSummoned/SkeletonWarriorPlayerSummoned", "RaisedSkeletonWarriors", Some("ActiveSkeletonLimit")),
    ("Hellhound/HellhoundPlayerSummoned", "SummonedHellhound", None),
    ("LeagueAncestral/AncestralTurtleTotemSpirit", "AncestralSpiritTurtle", None),
    ("LeagueAncestral/AncestralJadeHulkTotemSpirit", "AncestralSpiritHulk", None),
    ("LeagueAncestral/AncestralSpiritCasterTotemSpirit", "AncestralSpiritCaster", None),
    ("LeagueAncestral/AncestralKaruiHornTotemSpirit", "AncestralSpiritWarhorn", None),
    ("SkeletalConstruct/BoneConstructPlayerSummoned", "UnearthBoneConstruct", Some("ActiveUnearthBoneConstructLimit")),
    ("RhoaPlayerSummoned/RhoaPlayerSummoned", "SummonedRhoa", None),
    ("AnimatedItem/AnimatedWeaponPlayerSummoned", "ManifestWeapon", None),
    ("Daemon/RavenousSwarmPlayer/RavenousSwarmPlayerSummoned", "RavenousSwarm", None),
    ("Monsters/LivingLightningPlayerSummoned", "LivingLightning", Some("ActiveLivingLightningLimit")),
    ("Daemon/ArtilleryPlayer/SummonArtilleryDaemon", "TacticianMinion", None),
    ("HyenaMonster/HyenaCompanionPlayerSummoned", "HyenaMinion", Some("HyenaLimit")),
    ("Wolves/WolfCompanionPlayerSummoned", "WolfMinion", Some("WolfLimit")),
    ("Monsters/CorpseBeetlePlayerSummoned", "BeetleMinion", Some("BeetleLimit")),
    ("MarakethWaterDjinn/WaterDjinn", "WaterDjinn", None),
    ("MarakethTimeDjinn/FireDjinn", "FireDjinn", None),
    ("MarakethSamdDjinn/SandDjinn", "SandDjinn", None),
    ("Daemon/TangmazuRavensDaemon/TangmazuRavensDaemon", "MistRaven", None),
    ("UniqueItemMonsters/AzmerianSwarmMinion/AzmerianSwarmMinion", "AzmerianSwarm", Some("AzmerianSwarmLimit")),
    ("UniqueItemMonsters/AzmerianPackleader/PackleaderWolfMinion", "AzmerianWolf", None),
    ("CompanionBear/CompanionBearPlayerSummoned", "BearCompanion", None),
    ("LeagueExpeditionNew/PlayerSummoned/WardboundMinionPlayerSummoned", "Wardbound", Some("WardboundLimit")),
];

/// The same for PoE 1.
const POE1_MINIONS: [(&str, &str, Option<&str>); 63] = [
    ("RaisedZombies/RaisedZombieStandard", "RaisedZombie", Some("ActiveZombieLimit")),
    ("ChaosElemental/ChaosElementalSummoned", "SummonedChaosGolem", Some("ActiveGolemLimit")),
    ("FireElemental/FireElementalSummoned", "SummonedFlameGolem", Some("ActiveGolemLimit")),
    ("IceElemental/IceElementalSummoned", "SummonedIceGolem", Some("ActiveGolemLimit")),
    ("LightningGolem/LightningGolemSummoned", "SummonedLightningGolem", Some("ActiveGolemLimit")),
    ("RockGolem/RockGolemSummoned", "SummonedStoneGolem", Some("ActiveGolemLimit")),
    ("SummonedSkull/SummonedSkull", "SummonedRagingSpirit", Some("ActiveRagingSpiritLimit")),
    ("SummonedSkull/SpectralSummonedSkull", "SummonedEssenceSpirit", None),
    ("SummonedWolf/SummonedWolf", "SummonedSpectralWolf", Some("ActiveWolfLimit")),
    ("SummonedTiger/SummonedTiger", "SummonedSpectralTiger", Some("ActiveTigerLimit")),
    ("RaisedSkeletons/RaisedSkeletonStandard", "RaisedSkeleton", Some("ActiveSkeletonLimit")),
    ("RaisedSkeletons/RaisedSkeletonSpellcaster1Army", "RaisedSkeletonCaster", Some("ActiveSkeletonLimit")),
    ("RaisedSkeletons/RaisedSkeletonMelee1Army", "RaisedSkeletonMeleeVaal", Some("ActiveSkeletonLimit")),
    ("RaisedSkeletons/RaisedSkeletonRanged1Army", "RaisedSkeletonArcherVaal", Some("ActiveSkeletonLimit")),
    ("RaisedSkeletons/RaisedSkeletonRanged1Quality", "RaisedSkeletonArcher", Some("ActiveSkeletonLimit")),
    ("Clone/MarauderClone", "Clone", None),
    ("Clone/MarauderCloneImmobile", "ArrowClone", None),
    ("Clone/MarauderCloneImmobileRainOfArrows", "ArrowCloneRoA", None),
    ("Clone/MarauderCloneImmobileElementalShot", "ArrowCloneEle", None),
    ("SummonedSpider/SummonedSpider", "SpiderMinion", Some("ActiveSpiderLimit")),
    ("AnimatedItem/AnimatedWeapon", "AnimatedWeapon", Some("ActiveAnimatedWeaponLimit")),
    ("AnimatedItem/AnimatedArmour", "AnimatedArmour", None),
    ("IcyRagingSpirit/IcyRagingSpirit", "IcyRagingSpirit", None),
    ("AnimatedItem/UniqueAnimatedWeapon", "UniqueAnimatedWeapon", Some("ActiveAnimatedWeaponLimit")),
    ("SummonedPhantasm/SummonedPhantasm", "SummonedPhantasm", Some("ActivePhantasmLimit")),
    ("SpiderPlated/HeraldOfAgonySpiderPlated", "HeraldOfAgonySpiderPlated", None),
    ("Axis/AxisEliteSoldierHeraldOfLight", "AxisEliteSoldierHeraldOfLight", Some("ActiveSentinelOfPurityLimit")),
    ("AnimatedItem/HolyLivingRelic", "HolyLivingRelic", Some("ActiveHolyRelicLimit")),
    ("Axis/AxisEliteSoldierDominatingBlow", "AxisEliteSoldierDominatingBlow", Some("ActiveSentinelOfDominanceLimit")),
    ("Axis/AxisEliteSoldierDominatingBlowVaal", "AxisEliteSoldierDominatingBlowVaal", Some("ActiveSentinelOfDominanceLimit")),
    ("TemplarJudge/AbsolutionTemplarJudge", "AbsolutionTemplarJudge", Some("ActiveSentinelOfAbsolutionLimit")),
    ("TemplarJudge/AbsolutionTemplarJudgeVaal", "AbsolutionTemplarJudgeVaal", Some("ActiveSentinelOfAbsolutionLimit")),
    ("Rhoas/RhoaUniqueSummoned", "RhoaUniqueSummoned", Some("ActiveBeastMinionLimit")),
    ("Snake/SnakeSpitUniqueSummoned", "SnakeSpitUniqueSummoned", Some("ActiveBeastMinionLimit")),
    ("DropBear/DropBearUniqueSummoned", "DropBearUniqueSummoned", Some("ActiveBeastMinionLimit")),
    ("BoneGolem/BoneGolem", "SummonedCarrionGolem", Some("ActiveGolemLimit")),
    ("Skitterbot/SkitterbotCold", "SkitterbotCold", None),
    ("Skitterbot/SkitterbotLightning", "SkitterbotLightning", None),
    ("Skitterbot/SkitterbotFire", "SkitterbotFire", None),
    ("Skitterbot/AbyssCurseBot", "AbyssCurseBot", None),
    ("Skitterbot/AbyssDebuffBot", "AbyssDebuffBot", None),
    ("SummonedReaper/SummonedReaper", "SummonedReaper", Some("ActiveReaperLimit")),
    ("LeagueExpedition/Arbalest/SummonedArbalest", "SummonedArbalists", Some("ActiveArbalistLimit")),
    ("Axis/AxisEliteSoldierRadiance", "GuardianSentinel", None),
    ("AnimatedItem/ElementalLivingRelicFire", "GuardianRelicFire", None),
    ("AnimatedItem/ElementalLivingRelicCold", "GuardianRelicCold", None),
    ("AnimatedItem/ElementalLivingRelicLightning", "GuardianRelicLightning", None),
    ("ElderTentacle/ElderTentacleMinionLargePlayer", "VoidSpawn", Some("ActiveVoidSpawnLimit")),
    ("LeagueAncestral/AncestralAhuanaMinion", "AncestralAhuanaMinion", None),
    ("LeagueAncestral/AncestralAkoyaMinion", "AncestralAkoyaMinion", None),
    ("LeagueAncestral/AncestralIkiahoMinion", "AncestralIkiahoMinion", None),
    ("LeagueAncestral/AncestralKahuturoaMinion_", "AncestralKahuturoaMinion", None),
    ("LeagueAncestral/AncestralKaomMinion_", "AncestralKaomMinion", None),
    ("LeagueAncestral/AncestralKiloavaMinion__", "AncestralKiloavaMinion", None),
    ("LeagueAncestral/AncestralMaataMinion", "AncestralMaataMinion", None),
    ("LeagueAncestral/AncestralRakiataMinion", "AncestralRakiataMinion", None),
    ("LeagueAncestral/AncestralTawhanukuMinion", "AncestralTawhanukuMinion", None),
    ("LeagueAncestral/AncestralUtulaMinion_", "AncestralUtulaMinion", None),
    ("LivingLightning/LivingLightningSummoned", "LivingLightningMinion", Some("ActiveLivingLightningLimit")),
    ("SummonedPhantasm/SummonedPhantasmPenanceMark", "PenanceMarkPhantasm", None),
    ("AnimatedItem/HolyStrikeAnimatedWeapon", "HolyStrikeMinion", Some("ActiveHolyStrikeMinionLimit")),
    ("Breach/BreachFodderHandSpiderGraftSummoned", "Hiveborn", Some("ActiveHivebornLimit")),
    ("RaisedZombies/RestlessDeadPlayerSummoned", "ShamblingUndead", Some("ShamblingUndeadLimit")),
];

/// PoE 1 minions PoB marks `#hostile true`.
const POE1_HOSTILE: [&str; 1] = ["PenanceMarkPhantasm"];

/// The spectres PoB's PoE 1 `Spectres.txt` picks by hand, as
/// `folder: name name …` under `Metadata/Monsters/` unless the folder says
/// otherwise. PoE 1 has no rule to pick them by: SpectreOverrides alone names
/// 567 spectre varieties, of which PoB models 38.
const POE1_SPECTRES: &str = "
    Metadata/Monster/CageSpider: CageSpider2
    Metadata/Monster/KitavaDemon: KitavaDemon
    AtlasExiles/AdjudicatorInfluenceMonsters: AdjudicatorGrandMasterSpectre
    AtlasExiles/CrusaderInfluenceMonsters: CrusaderMageguardCasterSpectre CrusaderBlessedSisterSpectre
    AtlasExiles/CrusaderInfluenceMonsters: CrusaderTemplarJudgeSpectre
    AtlasExiles/EyrieInfluenceMonsters: EyrieSeraphArcherSpectre EyrieSeraphFighterSpectre_
    AtlasExiles/EyrieInfluenceMonsters: EyrieKiwethSpectre EyrieArmouredBirdSpectre__
    Axis: AxisCaster AxisCasterArc AxisCasterLunaris AxisEliteSoldier3Champion AxisExperimenter
    Axis: AxisExperimenter2 AxisExperimenterRaiseZombie
    Bandit: DockworkerChampion_
    Bandits: BanditBowExplosiveArrow BanditBowPoisonArrow BanditMeleeWarlordsMarkMaul BanditBowChampion
    Bandits: BanditRangedTornadoShotPetrified
    Beasts: BeastCaveDegenAura BeastVulnerabilityCurse BeastCleaveEnduringCry
    BloodChieftain: MonkeyChiefBloodEnrage MonkeyChiefBloodParasite
    BoneStalker: BoneStalker
    Bull: Bull
    Cannibal: CannibalMaleChampion
    DemonFemale: DemonFemale
    DemonModular: DemonFemaleRanged DemonFemaleRanged2 DemonModularBladeVortex DemonModularFire
    FaridunLeague/FaridunWarlock: FaridunWarlockLow FaridunWarlockMid_ FaridunWarlockHigh
    Frog: Frog Frog2
    GemMonster: Iguana IguanaChrome
    GhostPirates: GhostPirateBlackBowMaps GhostPirateBlackFlickerStrikeMaps GhostPirateGreenBladeVortex
    Goatman: GoatmanLeapSlam GoatmanLightningLeapSlamMaps GoatmanShamanFireball GoatmanShamanFireChampion
    Goatman: GoatmanShamanLightning MountainGoatmanChampion MountainGoatmanShamanIceSpear
    Grappler: Grappler GrapplerLabyrinth
    Guardians: GuardianFire GuardianFire_BlueMaps GuardianLightning
    HalfSkeleton: HalfSkeleton
    Hellion: Hellion3Spectre HellionBreachHowlSpectre
    HolyFireElemental: HolyFireElementalSolarisBeam
    InsectSpawner: InsectSpawner
    KaomWarrior: KaomWarrior2 KaomWarrior3 KaomWarrior7
    KitavaCultist: VaalCultistSpearBloodDelve VaalCultistSpearBloodChampionDelve VaalCultistSpearChaosDelve
    KitavaCultist: VaalCultistSpearChaosChampionDelve VaalCultistSpearFireDelve
    KitavaCultist: VaalCultistSpearFireChampionDelve_ VaalCultistSpearLightningDelve
    KitavaCultist: VaalCultistSpearLightningChampionDelve_
    Kiweth: Kiweth KiwethSeagull
    LeagueAzmeri/SpecialCorpses: RobotArgusLow RobotArgusMid RobotArgusHigh__ KudukuLow KudukuMid KudukuHigh
    LeagueAzmeri/SpecialCorpses: AdmiralLow AdmiralMid__ AdmiralHigh_ AnimatedSwordLow AnimatedSwordMid
    LeagueAzmeri/SpecialCorpses: AnimatedSwordHigh_ BarrageDemonLow BarrageDemonMid BarrageDemonHigh_
    LeagueAzmeri/SpecialCorpses: BasaliskLow BasaliskMid BasaliskHigh CasterDemonLow CasterDemonMid
    LeagueAzmeri/SpecialCorpses: CasterDemonHigh CycloneDemonLow CycloneDemonMid CycloneDemonHigh
    LeagueAzmeri/SpecialCorpses: DeathKnightLow DeathKnightMid DeathKnightHigh DualstrikeDemonLow
    LeagueAzmeri/SpecialCorpses: DualstrikeDemonMid DualstrikeDemonHigh FlaskloverLow__ FlaskloverMid
    LeagueAzmeri/SpecialCorpses: FlaskloverHigh ForgeHoundLow ForgeHoundMid ForgeHoundHigh_ GeofriLow
    LeagueAzmeri/SpecialCorpses: GeofriMid_ GeofriHigh GoddessLow GoddessMid GoddessHigh HarvestBirdLow
    LeagueAzmeri/SpecialCorpses: HarvestBirdMid HarvestBirdHigh ManaPhantasmLow__ ManaPhantasmMid
    LeagueAzmeri/SpecialCorpses: ManaPhantasmHigh MegaSkeletonLow MegaSkeletonMid MegaSkeletonHigh OakLow
    LeagueAzmeri/SpecialCorpses: OakMid OakHigh ReaperLow ReaperMid ReaperHigh ShepherdLow ShepherdMid_
    LeagueAzmeri/SpecialCorpses: ShepherdHigh SpiderLeaderLow SpiderLeaderMid SpiderLeaderHigh_
    LeagueAzmeri/SpecialCorpses: TankyZombieLow TankyZombieMid TankyZombieHigh TentacleMinionLow
    LeagueAzmeri/SpecialCorpses: TentacleMinionMid TentacleMinionHigh TigerLow TigerMid TigerHigh TurtleLow
    LeagueAzmeri/SpecialCorpses: TurtleMid_ TurtleHigh VaalOversoulLow VaalOversoulMid VaalOversoulHigh
    LeagueAzmeri/SpecialCorpses: VikingLow VikingMid VikingHigh SlammerDemonLow SlammerDemonMid
    LeagueAzmeri/SpecialCorpses: SlammerDemonHigh FlameblasterLow_ FlameblasterMid_ FlameblasterHigh_
    LeagueAzmeri/SpecialCorpses: DemonBossLow DemonBossMid DemonBossHigh SynthesisGolemLow SynthesisGolemMid
    LeagueAzmeri/SpecialCorpses: SynthesisGolemHigh
    LeagueAzmeri/SpecialCorpses/Firefury: FirefuryLow FirefuryMid FirefuryHigh_
    LeagueAzmeri/SpecialCorpses/Hailrake: HailrakeLow HailrakeMid HailrakeHigh
    LeagueAzmeri/SpecialCorpses/Hydra: HydraLow HydraMid HydraHigh_
    LeagueAzmeri/SpecialCorpses/Mannequin: MannequinLow MannequinMid MannequinHigh_
    LeagueBetrayal: BetrayalSecretPolice2Spectre_
    LeagueCrucible/Cold: Pyromaniac
    LeagueCrucible/Lightning: Vendigo_
    LeagueDelve: ProtoVaalWarriorElite
    LeagueDelve/GhostEncounter: WraithPurple Wraith
    LeagueHarvest/Blue: HarvestNessaCrabT3Spectre HarvestRhexT3Spectre
    LeagueHarvest/Red: HarvestMinerHammerT2Spectre
    LeagueHeist/Robot: RobotClockworkGolemColdSpectre RobotPyreKnightEliteSpectre
    LeagueHeist/Science: ProjectUnarmedEliteGuardSpectre
    LeagueHeist/Thug: ThugRanged1EliteSpectre
    LeagueHellscape/DemonFaction: HellscapeDemonElite1Spectre HellscapeDemonElite2_Spectre
    LeagueHellscape/FleshFaction: HellscapeFleshFodder4Spectre HellscapeFleshElite1Spectre
    LeagueHellscape/PaleFaction: HellscapePaleElite1Spectre HellscapePaleElite2Spectre
    LeagueSynthesis: SynthesisSoulstealer3Spectre SynthesisSoulstealer4Spectre
    LeagueUltimatum/Guard: GuardBowColdWeakSpectre GuardBowColdSpectre
    LegionLeague: LegionTemplarCaster1Spectre LegionKaruiArcherSpectre LegionKaruiMeleeFireSpectre
    LegionLeague: LegionTemplarMelee2Spectre
    Lion: LionDesertSkinPuncture LionWolf3Champion
    Maligaro: SecretDesecrateMonster
    MassSkeleton: MassSkeleton
    Miner: MinerLantern MinerLanternCrystalVeins
    MinerLarge: MinerLargeCommanderBreachSpectre
    Monkeys: FlameBearer
    MossMonster: FireMonster
    MotherOfFlames: MotherOfFlamesZombie
    Necromancer: NecromancerConductivity NecromancerEnfeebleCurse NecromancerFlamability
    Necromancer: NecromancerFrostbite NecromancerElementalWeakness NecromancerProjectileWeakness
    Necromancer: NecromancerVulnerability
    Pyromaniac: PyromaniacFire PyromaniacPoison
    Revenant: Revenant RevenantMapBossStandalone_AtlasUber
    SandLeaper: SandLeaperBreachSpectre_
    Seawitch: SeaWitchFrostBolt SeaWitchScreech SeaWitchSpawnExploding SeaWitchSpawnTemporalChains
    Seawitch: SeaWitchVulnerabilityCurse
    SkeletonCannon: SkeletonCannon1
    Skeletons: SkeletonBowPuncture SkeletonBowLightning SkeletonMeleeLarge SkeletonBowLightning3
    Skeletons: SkeletonCasterColdMultipleProjectiles SkeletonCasterFireMultipleProjectiles2
    Skeletons: SkeletonBowPoison SkeletonBowLightning2 SkeletonBowLightning4 SkeletonCasterLightningSpark
    Skeletons: SkeletonBlackCaster1_ SkeletonBowProjectileWeaknessCurse
    Skeletons: SkeletonMeleeKnightElementalSwordIncursionChampion SkeletonBowKnightElemental
    Skeletons: SkeletonMeleeBlackAbyssBoneLance
    Snake: SnakeMeleeSpit SnakeScorpionMultiShot
    SpiderPlated: SpiderPlatedUnholyEmerge
    Spiders: SpiderThornFlickerStrike SpiderThornViperStrikeFlickerStrike DelveSpiderPacksMediumSpectre
    Statue: DaressoStatueLargeMaleSpear StoneStatueMaleBow
    Taster: Taster
    TemplarSlaveDriver: TemplarSlaveDriver TemplarSlaveDriverKitava
    Undying: CityStalkerMaleCasterArmour UndyingOutcastPuncture UndyingOutcastWhirlingBlades
    VaalMonsters: VaalOverseer
    WickerMan: WickerMan
    incaminion: Fragment
";

/// Full ids of [`POE1_SPECTRES`].
fn poe1_spectre_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for line in POE1_SPECTRES.lines() {
        let Some((folder, names)) = line.trim().split_once(':') else { continue };
        let folder = match folder.starts_with("Metadata/") {
            true => folder.to_string(),
            false => format!("{}{}", MONSTERS, folder),
        };
        ids.extend(names.split_whitespace().map(|name| format!("{}/{}", folder, name)));
    }
    ids
}

const MONSTERS: &str = "Metadata/Monsters/";

/// PoB's weapon type for a monster's item class, PoE 2.
const POE2_WEAPONS: [(&str, &str); 21] = [
    ("Claw", "Claw"),
    ("Dagger", "Dagger"),
    ("Wand", "Wand"),
    ("One Hand Sword", "One Hand Sword"),
    ("Thrusting One Hand Sword", "One Hand Sword"),
    ("One Hand Axe", "One Hand Axe"),
    ("One Hand Mace", "One Hand Mace"),
    ("Crossbow", "Crossbow"),
    ("Bow", "Bow"),
    ("Fishing Rod", "Fishing Rod"),
    ("Staff", "Staff"),
    ("Warstaff", "Warstaff"),
    ("Two Hand Sword", "Two Hand Sword"),
    ("Two Hand Axe", "Two Hand Axe"),
    ("Two Hand Mace", "Two Hand Mace"),
    ("Shield", "Shield"),
    ("Sceptre", "One Hand Mace"),
    ("Flail", "Flail"),
    ("Spear", "Spear"),
    ("Talisman", "Talisman"),
    ("Unarmed", "None"),
];

/// PoE 1.
const POE1_WEAPONS: [(&str, &str); 16] = [
    ("Claw", "Claw"),
    ("Dagger", "Dagger"),
    ("Wand", "Wand"),
    ("One Hand Sword", "One Handed Sword"),
    ("Thrusting One Hand Sword", "One Handed Sword"),
    ("One Hand Axe", "One Handed Axe"),
    ("One Hand Mace", "One Handed Mace"),
    ("Bow", "Bow"),
    ("Fishing Rod", "Fishing Rod"),
    ("Staff", "Staff"),
    ("Two Hand Sword", "Two Handed Sword"),
    ("Two Hand Axe", "Two Handed Axe"),
    ("Two Hand Mace", "Two Handed Mace"),
    ("Shield", "Shield"),
    ("Sceptre", "One Handed Mace"),
    ("Unarmed", "None"),
];

/// `Minions.lua` and `Spectres.lua`. Player minions are keyed by PoB's name
/// for them; any other variety a player skill summons comes out under its
/// game id. PoE 2's spectres are the ones `spectreList.lua` picks from the
/// game; PoE 1's are PoB's hand-picked list.
pub fn minions(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let monsters = Monsters::new(ctx)?;
    let mut out = Table::new();
    for (key, id, limit) in minion_list(ctx, &monsters) {
        let Some(row) = monsters.varieties.by_id(&id) else { continue };
        let mut entry = monsters.entry(ctx, row);
        entry.set_opt("limit", limit);
        if game == Game::Poe1 && POE1_HOSTILE.contains(&key.as_str()) {
            entry.set("hostile", true);
        }
        out.set(key, entry);
    }
    write(ctx, "Minions", out)?;

    let mut spectres = Table::new();
    for id in spectre_ids(ctx, &monsters)? {
        if let Some(row) = monsters.varieties.by_id(&id) {
            spectres.set(id.clone(), monsters.entry(ctx, row));
        }
    }
    write(ctx, "Spectres", spectres)
}

/// The varieties `Minions.lua` and `Spectres.lua` hold, in that order, for
/// the skill files that collect their skills.
pub(crate) fn monster_ids(ctx: &Ctx) -> Result<(Vec<String>, Vec<String>), String> {
    let monsters = Monsters::new(ctx)?;
    let minions = minion_list(ctx, &monsters).into_iter().map(|(_, id, _)| id).collect();
    let spectres =
        spectre_ids(ctx, &monsters)?.into_iter().filter(|id| monsters.varieties.by_id(id).is_some()).collect();
    Ok((minions, spectres))
}

/// Player minions as `Minions.lua` keys them: PoB's name, or the game id for
/// one PoB does not name, with the variety id and PoB's limit.
fn minion_list(ctx: &Ctx, monsters: &Monsters) -> Vec<(String, String, Option<&'static str>)> {
    let named: Vec<(String, &'static str, Option<&'static str>)> = match game(ctx) {
        Game::Poe2 => POE2_MINIONS.iter().map(|(id, name, limit)| (format!("{}{}", MONSTERS, id), *name, *limit)).collect(),
        Game::Poe1 => POE1_MINIONS.iter().map(|(id, name, limit)| (format!("{}{}", MONSTERS, id), *name, *limit)).collect(),
    };
    let mut out: Vec<(String, String, Option<&'static str>)> = named
        .iter()
        .filter(|(id, _, _)| monsters.varieties.by_id(id).is_some())
        .map(|(id, name, limit)| (name.to_string(), id.clone(), *limit))
        .collect();
    let known: HashSet<&str> = named.iter().map(|(id, _, _)| id.as_str()).collect();
    let mut names: HashSet<String> =
        named.iter().filter_map(|(id, _, _)| monsters.varieties.by_id(id)).map(|row| row.string("Name")).collect();
    for id in monsters.summoned_by_players(ctx) {
        let Some(row) = monsters.varieties.by_id(&id) else { continue };
        // Another skin of a minion already listed adds nothing.
        if known.contains(id.as_str()) || !names.insert(row.string("Name")) {
            continue;
        }
        out.push((id.clone(), id, None));
    }
    out
}

/// The spectre ids for this game, in the order PoB writes them.
fn spectre_ids(ctx: &Ctx, monsters: &Monsters) -> Result<Vec<String>, String> {
    match game(ctx) {
        Game::Poe2 => {
            let mut ids = monsters.spectre_list(ctx)?;
            ids.extend(POE2_EXTRA_SPECTRES.iter().map(|id| format!("{}{}", MONSTERS, id)));
            Ok(ids)
        }
        Game::Poe1 => Ok(poe1_spectre_ids()),
    }
}

/// Varieties PoB's PoE 2 `Spectres.txt` adds by hand: each shares its name,
/// spirit cost and skills with one `spectreList.lua` keeps, so the list drops
/// it as a repeat.
const POE2_EXTRA_SPECTRES: [&str; 11] = [
    "NettleAnt/NettleAntSummoned",
    "SaltGolem/SaltGolem_",
    "Skeletons/Basic/GraveSkeletonBowHusbandWife",
    "Skeletons/Basic/GraveSkeletonCasterColdHusbandWife",
    "Skeletons/Basic/GraveSkeletonOneHandSwordHusbandWife",
    "Skeletons/Basic/GraveSkeletonOneHandSwordShield",
    "Skeletons/Basic/GraveSkeletonOneHandSwordShieldHusbandWife",
    "Skeletons/Basic/GraveSkeletonOneHandSword__",
    "Skeletons/Basic/GraveSkeletonUnarmedHusbandWife",
    "Skeletons/Basic/GraveSkeletonUnarmedStance2",
    "TerracottaGuardians/TerracottaGuardianSceptreAmbush__",
];

/// The columns `minions.lua` and `spectreList.lua` read from
/// `MonsterVarieties` and `MonsterTypes`, under whichever names the schema
/// gives them.
struct Monsters {
    game: Game,
    varieties: Rc<LoadedTable>,
    kind: &'static str,
    mods: &'static str,
    mods2: Option<&'static str>,
    tags: &'static str,
    effects: &'static str,
    main_hand: &'static str,
    off_hand: &'static str,
    ai: Option<&'static str>,
    /// Unnamed in dat-schema: the bool five columns after
    /// `SinkAnimation_AOFile`, `NotSpectre` in PoB's spec.
    not_spectre: Option<usize>,
    player_minion: &'static str,
    energy_shield: &'static str,
    resistances: &'static str,
    /// Unnamed in PoE 2's dat-schema: the bool after `MonsterResistances`.
    ignores_attack_speed: Option<usize>,
    spawns: Option<Spawns>,
}

impl Monsters {
    fn new(ctx: &Ctx) -> Result<Self, String> {
        let game = game(ctx);
        let varieties = ctx.table("MonsterVarieties")?;
        let types = ctx.table("MonsterTypes")?;
        let v = &varieties;
        let resistances = types.require(&["MonsterResistances", "Resistances"])?;
        Ok(Self {
            game,
            kind: v.require(&["MonsterType", "MonsterTypesKey"])?,
            mods: v.require(&["Mods", "ModsKeys"])?,
            mods2: v.pick(&["Mods2", "ModsKeys2"]),
            tags: v.require(&["Tags", "TagsKeys"])?,
            effects: v.require(&["GrantedEffects", "GrantedEffectsKeys"])?,
            main_hand: v.require(&["MainHand_ItemClass", "MainHand_ItemClassesKey"])?,
            off_hand: v.require(&["OffHand_ItemClass", "OffHand_ItemClassesKey"])?,
            ai: v.pick(&["AISFile"]),
            not_spectre: v.column_or_after(&["NotSpectre"], "SinkAnimation_AOFile", 5),
            player_minion: types.require(&["IsSummoned", "IsPlayerMinion"])?,
            energy_shield: types.require(&["EnergyShieldFromLife", "EnergyShield"])?,
            resistances,
            ignores_attack_speed: types.column_or_after(&["BaseDamageIgnoresAttackSpeed"], resistances, 1),
            spawns: match game {
                Game::Poe2 => Some(Spawns::new(ctx)?),
                Game::Poe1 => None,
            },
            varieties,
        })
    }

    /// One `minions[…] = { … }` block as `#emit` writes it, less `modList`
    /// (PoB's SkillStatMap) and the directive file's `#flags`, `#mod` and
    /// `#skill` lines.
    fn entry(&self, ctx: &Ctx, row: Row<'_>) -> Table {
        let poe2 = self.game == Game::Poe2;
        let mut e = Table::new();
        e.set("name", lua_literal(row.str("Name")));
        e.set("monsterTags", Table::list(ctx.rr.deref_list_ids(row, self.tags).iter().map(|t| lua_literal(t))));
        e.set("life", row.int("LifeMultiplier") as f64 / 100.0);
        if let Some(kind) = ctx.rr.deref(row, self.kind) {
            let t = kind.row();
            if self.ignores_attack_speed.is_some_and(|c| t.bool_at(c)) {
                e.set("baseDamageIgnoresAttackSpeed", true);
            }
            if !poe2 {
                if t.bool("AltLife1") {
                    e.set("lifeScaling", "AltLife1");
                }
                if t.bool("AltLife2") {
                    e.set("lifeScaling", "AltLife2");
                }
            }
            let es = t.int(self.energy_shield) as f64;
            if es != 0.0 {
                e.set("energyShield", if poe2 { es / 100.0 } else { 0.4 * es / 100.0 });
            }
            for (key, col) in [("armour", "Armour"), ("evasion", "Evasion")] {
                let value = t.int(col) as f64;
                if value != 0.0 {
                    e.set(key, value / 100.0);
                }
            }
            self.set_resistances(ctx, t, &mut e);
            e.set("damageSpread", t.int("DamageSpread") as f64 / 100.0);
            if !poe2 {
                e.set("accuracy", t.int("Accuracy") as f64 / 100.0);
            }
        }
        e.set("damage", row.int("DamageMultiplier") as f64 / 100.0);
        e.set("attackTime", row.int("AttackSpeed") as f64 / 1000.0);
        e.set("attackRange", row.int("MaximumAttackDistance"));
        if poe2 {
            e.set("accuracy", 1);
            e.set("critChance", round(row.float("AttackCrit") as f64 * 100.0, Some(2)));
        }
        for id in ctx.rr.deref_list_ids(row, self.mods) {
            let fixup = match id.as_str() {
                "MonsterSpeedAndDamageFixupSmall" => 0.11,
                "MonsterSpeedAndDamageFixupLarge" => 0.22,
                "MonsterSpeedAndDamageFixupComplete" => 0.33,
                _ => continue,
            };
            e.set("damageFixup", fixup);
        }
        let weapons: &[(&str, &str)] = if poe2 { &POE2_WEAPONS } else { &POE1_WEAPONS };
        for (key, col) in [("weaponType1", self.main_hand), ("weaponType2", self.off_hand)] {
            let class = ctx.rr.deref_id(row, col);
            if let Some((_, name)) = class.and_then(|c| weapons.iter().find(|(id, _)| *id == c)) {
                e.set(key, *name);
            }
        }
        if poe2 {
            e.set("baseMovementSpeed", row.int("MovementSpeed"));
            let experience = row.int("ExperienceMultiplier") as f64 / 100.0;
            e.set("spectreReservation", (experience.powf(0.75) * 50.0).floor());
            e.set("companionReservation", (experience.sqrt() * 100.0).floor() / 100.0 * 30.0);
            if let Some(category) = ctx.rr.deref(row, "MonsterCategory") {
                e.set("monsterCategory", lua_literal(category.row().str("Name")));
            }
            let locations = self.spawns.as_ref().map(|s| s.locations(row.str("Name"))).unwrap_or_default();
            e.set("spawnLocation", Table::list(locations.iter().map(|l| lua_literal(l))));
        }
        e.set("skillList", Table::list(ctx.rr.deref_list_ids(row, self.effects).iter().map(|s| lua_literal(s))));
        e
    }

    /// PoE 2 reads the first entry of the first resistance row's normal and
    /// rare lists; PoE 1 its merciless columns. A value PoB would fail on is
    /// left out.
    fn set_resistances(&self, ctx: &Ctx, kind: Row<'_>, e: &mut Table) {
        const ELEMENTS: [(&str, &str); 4] = [("fire", "Fire"), ("cold", "Cold"), ("lightning", "Lightning"), ("chaos", "Chaos")];
        match self.game {
            Game::Poe2 => {
                let Some(res) = ctx.rr.deref_list(kind, self.resistances).into_iter().next() else { return };
                let r = res.row();
                for (key, element) in ELEMENTS {
                    let first = |col: String| r.list_int(&col).first().copied();
                    e.set_opt(format!("{}Resist", key), first(format!("{}1", element)));
                    let companion = format!("companion{}{}Resist", &element[..1], &element[1..]);
                    e.set_opt(companion, first(format!("{}3", element)));
                }
            }
            Game::Poe1 => {
                let Some(res) = ctx.rr.deref(kind, self.resistances) else { return };
                for (key, element) in ELEMENTS {
                    e.set(format!("{}Resist", key), res.row().int(&format!("{}Merciless", element)));
                }
            }
        }
    }

    /// Varieties a player skill summons that PoB has no name for: those the
    /// game's `DisplayMinionMonsterType` names, and those of a player-minion
    /// type whose id says `PlayerSummoned`. In table order, without repeats.
    fn summoned_by_players(&self, ctx: &Ctx) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        if let Some(display) = ctx.optional_table("DisplayMinionMonsterType") {
            if let Some(col) = display.pick(&["MonsterVarietiesKey", "MonsterVariety"]) {
                ids.extend(display.rows().filter_map(|row| ctx.rr.deref_id(row, col)));
            }
        }
        for row in self.varieties.rows() {
            if row.id().contains("PlayerSummoned") && self.is_player_minion(ctx, row) {
                ids.push(row.id().to_string());
            }
        }
        let mut seen = HashSet::new();
        ids.retain(|id| seen.insert(id.clone()));
        ids
    }

    fn is_player_minion(&self, ctx: &Ctx, row: Row<'_>) -> bool {
        ctx.rr.deref(row, self.kind).is_some_and(|t| t.row().bool(self.player_minion))
    }

    /// `spectreList.lua`: every variety that can be raised as a spectre,
    /// replaced by its `SpectreOverrides` variety, one per name, spirit cost
    /// and skill set.
    fn spectre_list(&self, ctx: &Ctx) -> Result<Vec<String>, String> {
        let not_spectre = self.not_spectre.ok_or("MonsterVarieties has no NotSpectre column")?;
        let overrides = ctx.table("SpectreOverrides")?;
        let mut override_of: HashMap<String, usize> = HashMap::new();
        for row in overrides.rows() {
            let (Some(monster), Some(spectre)) = (ctx.rr.deref(row, "Monster"), row.key("Spectre")) else { continue };
            override_of.entry(monster.id()).or_insert(spectre);
        }
        const ID_EXCLUDED: [&str; 8] = ["TestSpawn", "Mtx", "InsectTest", "TEMP", "GenericTest", "PhysicsTest", "Royale", "NPC"];
        let mut signatures = HashSet::new();
        let mut out = Vec::new();
        for row in self.varieties.rows() {
            let id = row.id();
            let name = row.str("Name");
            let is_beast = ctx
                .rr
                .deref(row, "MonsterCategory")
                .is_some_and(|c| c.row().str("Name") == "Beast");
            let eligible = !row.bool_at(not_spectre)
                && (!row.bool("BossHealthBar") || is_beast)
                && !self.is_player_minion(ctx, row)
                && !ID_EXCLUDED.iter().any(|s| id.contains(s))
                && !["DNT", "Daemon", "Invisible"].iter().any(|s| name.contains(s))
                && !self.ai.is_some_and(|c| row.str(c).contains("NoAI"))
                && !ctx.rr.deref_list(row, self.effects).is_empty();
            if !eligible {
                continue;
            }
            let mut mods = ctx.rr.deref_list_ids(row, self.mods);
            if let Some(col) = self.mods2 {
                mods.extend(ctx.rr.deref_list_ids(row, col));
            }
            if mods.iter().any(|m| m == "CannotBeUsedAsMinion")
                || ctx.rr.deref_list_ids(row, self.tags).iter().any(|t| t == "unusable_corpse")
            {
                continue;
            }
            let source = override_of.get(id).and_then(|&i| self.varieties.row(i)).unwrap_or(row);
            let mut skills = ctx.rr.deref_list_ids(source, self.effects);
            skills.sort();
            let signature = format!("{}|{}|{}", name, source.int("ExperienceMultiplier"), skills.join(","));
            if signatures.insert(signature) {
                out.push(source.id().to_string());
            }
        }
        Ok(out)
    }
}

/// Where each PoE 2 monster can be met, by name, as `minions.lua` finds it:
/// the packs its name appears in, and the areas and endgame maps those
/// packs populate.
struct Spawns {
    packs_by_name: HashMap<String, Vec<String>>,
    areas_by_pack: HashMap<String, Vec<String>>,
}

impl Spawns {
    fn new(ctx: &Ctx) -> Result<Self, String> {
        let entries = ctx.table("MonsterPackEntries")?;
        let packs = ctx.table("MonsterPacks")?;
        let areas = ctx.table("WorldAreas")?;
        let maps = ctx.table("EndgameMaps")?;
        let pack_areas = packs.require(&["WorldAreas", "WorldAreasKeys"])?;
        let additional = packs.pick(&["AdditionalMonsters"]);
        let bosses = packs.require(&["BossMonsters", "BossMonster_MonsterVarietiesKeys"])?;
        let area_tags = areas.require(&["Tags", "TagsKeys"])?;
        let map_area = maps.require(&["WorldArea", "Id"])?;
        let map_packs = maps.require(&["MonsterPacks", "NativePacks"])?;

        let mut packs_by_name: HashMap<String, Vec<String>> = HashMap::new();
        let mut pack_ids: Vec<String> = Vec::new();
        let mut listed = HashSet::new();
        for entry in entries.rows() {
            let Some(pack) = ctx.rr.deref(entry, "MonsterPacksKey") else { continue };
            let pack_id = pack.id();
            if listed.insert(pack_id.clone()) {
                pack_ids.push(pack_id.clone());
            }
            if let Some(variety) = ctx.rr.deref(entry, "MonsterVarietiesKey") {
                packs_by_name.entry(variety.row().string("Name")).or_default().push(pack_id);
            }
        }
        for pack_id in &pack_ids {
            let Some(pack) = packs.by_id(pack_id) else { continue };
            for col in additional.iter().copied().chain([bosses]) {
                for monster in ctx.rr.deref_list(pack, col) {
                    packs_by_name.entry(monster.row().string("Name")).or_default().push(pack_id.clone());
                }
            }
        }

        let display = |area: Row<'_>, from_pack: bool| -> Option<String> {
            let name = area.str("Name");
            if name == "NULL" || name.contains("DNT") {
                return None;
            }
            let is_map = ctx.rr.deref_list_ids(area, area_tags).iter().any(|t| t == "map");
            let act = area.int("Act");
            let suffix = if is_map {
                " (Map)".to_string()
            } else if let Some(floor) = sanctum_floor(area.id()).filter(|_| from_pack) {
                format!(" (Floor {})", floor)
            } else if act != 10 && !(from_pack && name == "Trial of the Sekhemas") {
                format!(" (Act {})", act)
            } else {
                String::new()
            };
            Some(format!("{}{}", name, suffix))
        };
        let mut areas_by_pack: HashMap<String, Vec<String>> = HashMap::new();
        for pack_id in packs_by_name.values().flatten().collect::<HashSet<_>>() {
            let mut names = Vec::new();
            if let Some(pack) = packs.by_id(pack_id) {
                for area in ctx.rr.deref_list(pack, pack_areas) {
                    if let Some(area) = areas.by_id(area.row().id()) {
                        names.extend(display(area, true));
                    }
                }
            }
            areas_by_pack.insert(pack_id.clone(), names);
        }
        for map in maps.rows() {
            let Some(target) = ctx.rr.deref(map, map_area) else { continue };
            let Some(area) = areas.by_id(target.row().id()) else { continue };
            let Some(name) = display(area, false) else { continue };
            for pack_id in ctx.rr.deref_list_ids(map, map_packs) {
                if let Some(names) = areas_by_pack.get_mut(&pack_id) {
                    names.push(name.clone());
                }
            }
        }
        Ok(Self { packs_by_name, areas_by_pack })
    }

    /// The sorted place names for a monster name, with the Ziggurat standing
    /// in for every map.
    fn locations(&self, name: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for pack in self.packs_by_name.get(name).into_iter().flatten() {
            for place in self.areas_by_pack.get(pack).into_iter().flatten() {
                if seen.insert(place.as_str()) {
                    out.push(place.clone());
                }
            }
        }
        out.sort();
        for place in out.iter_mut() {
            if place == "The Ziggurat Refuge" {
                *place = "Found in Maps".to_string();
            }
        }
        out
    }
}

/// The digits of `^Sanctum_(%d+)`.
fn sanctum_floor(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("Sanctum_")?;
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    (digits > 0).then(|| &rest[..digits])
}

/// PoB's `Export/Enemies/Bosses.txt` (PoE 1): its name for each boss, the
/// boss's monster type, and whether PoB counts it as a pinnacle boss.
const BOSSES: [(&str, &str, bool); 22] = [
    ("Venarius", "SynthesisVenarius", true),
    ("EaterOfWorlds", "AtlasInvadersConsumeBoss", true),
    ("SearingExarch", "AtlasInvadersCleansingBoss", true),
    ("Maven", "TheMaven", true),
    ("Sirus", "AtlasExiles5", true),
    ("Shaper", "TheShaperBoss", true),
    ("Elder", "TheElder", true),
    ("BlackStar", "AtlasInvadersBlackStarBoss", false),
    ("InfiniteHunger", "AtlasInvadersDoomBoss", false),
    ("Atziri", "Atziri", false),
    ("Phoenix", "AtlasBossPhoenix", false),
    ("Hydra", "AtlasBossHydra", false),
    ("Minotaur", "AtlasBossMinotaur", false),
    ("Chimera", "AtlasBossChimera", false),
    ("Enslaver", "ElderGuardian1", false),
    ("Eradicator", "ElderGuardian2", false),
    ("Constrictor", "ElderGuardian3", false),
    ("Purifier", "ElderGuardian4", false),
    ("Baran", "AtlasExiles1", false),
    ("Veritania", "AtlasExiles2", false),
    ("AlHezmin", "AtlasExiles3", false),
    ("Drox", "AtlasExiles4", false),
];

/// PoB's `Export/Enemies/BossSkills.txt` less its `#tooltip` text: each
/// `#boss` line names the boss, its variety and whether the earlier and map
/// boss rules apply; each `#skill` line the skill's name, its granted effect
/// and PoB's tuning for it.
const BOSS_SKILLS: &str = "
#boss Atziri Metadata/Monsters/Atziri/Atziri true true
#skill Flameblast AtziriFlameblastEmpowered, stages = 10,
#boss Shaper Metadata/Monsters/AtlasBosses/TheShaperBoss false false
#skill Ball AtlasBossAcceleratingProjectiles
#skill Slam AtlasBossFlickerSlam, speedMult = 8775,
#skill Beam AtlasBossCelestialBeam, skillIndexUber = nil,
#boss Sirus Metadata/Monsters/AtlasExiles/AtlasExile5 false false
#skill Meteor AtlasExileOrionCircleMazeBlast3, skillIndex = 4,
#boss Cortex Metadata/Monsters/LeagueSynthesis/SynthesisVenariusBoss false true
#skill GroundDegen SynthesisVenariusQuicksand, skillIndexUber = nil, SkillExtraDamageMult = 226,
#boss Exarch Metadata/Monsters/AtlasInvaders/CleansingMonsters/CleansingBoss false false
#skill Ball CleansingFireWall, skillIndexUber = nil, speedMult = 4545,
#boss Eater Metadata/Monsters/AtlasInvaders/ConsumeMonsters/ConsumeBoss false false
#skill Beam GSConsumeBossDisintegrateBeam, skillIndexUber = nil,
#boss Maven Metadata/Monsters/MavenBoss/TheMaven false true
#skill Fireball MavenSuperFireProjectile, GrantedEffectId2 = MavenSuperFireProjectileImpact,
#skill MemoryGame MavenMemoryGame, skillIndexUber = nil,
";

/// Skills whose damage `bossData.lua` takes from base damage numbers written
/// into the script rather than from the game, so their damage multipliers
/// are left out.
const HAND_DAMAGE_SKILLS: [&str; 5] = [
    "AtlasBossFlickerSlam",
    "CleansingFireWall",
    "GSConsumeBossDisintegrateBeam",
    "MavenSuperFireProjectileImpact",
    "MavenMemoryGame",
];

/// The one of those whose uber multiplier PoB also writes by hand (201).
const HAND_UBER_SKILL: &str = "MavenSuperFireProjectileImpact";

const DAMAGE_TYPES: [&str; 5] =["Physical", "Lightning", "Cold", "Fire", "Chaos"];

/// `Bosses.lua` and `BossSkills.lua` (PoE 1; PoB-PoE2's `bossData.lua` is
/// switched off and ships PoE 1's files).
pub fn bosses(ctx: &Ctx) -> Result<(), String> {
    let types = ctx.table("MonsterTypes")?;
    let mut out = Table::new();
    for (name, id, uber) in BOSSES {
        let Some(kind) = types.by_id(id) else { continue };
        out.set(name, Table::new().with("armourMult", kind.int("Armour")).with("evasionMult", kind.int("Evasion")).with("isUber", uber));
    }
    write(ctx, "Bosses", out)?;
    write(ctx, "BossSkills", boss_skills(ctx)?)
}

/// One boss as its `#boss` line sets it up.
struct Boss {
    name: String,
    earlier_uber: bool,
    map_boss: bool,
    unique: bool,
}

/// What `bossData.lua` reads for one granted effect.
struct EffectLevels<'t> {
    effect: Row<'t>,
    stat_set: Option<Row<'t>>,
    levels: Vec<Row<'t>>,
}

fn boss_skills(ctx: &Ctx) -> Result<Table, String> {
    let varieties = ctx.table("MonsterVarieties")?;
    let effects = ctx.table("GrantedEffects")?;
    let stat_sets = ctx.table("GrantedEffectStatSets")?;
    let per_level = ctx.table("GrantedEffectStatSetsPerLevel")?;
    let mods = ctx.table("Mods")?;
    let difficulty = ctx.table("MonsterMapDifficulty")?;
    let monster_stats = ctx.table("DefaultMonsterStats")?;
    let level_effects = per_level.require(&["GrantedEffects", "GrantedEffect"])?;

    let stat1 = |id: &str| -> Option<f64> {
        let row = mods.by_id(id)?;
        match mods.pick(&["Stat1Min"]) {
            Some(col) => Some(row.int(col) as f64),
            None => row.interval("Stat1Value").map(|(min, _)| min as f64),
        }
    };
    let unique = 1.0 + stat1("MonsterUnique5").ok_or("Mods has no MonsterUnique5")? / 100.0;
    let unique_attack = unique * (1.0 - stat1("MonsterUnique8").ok_or("Mods has no MonsterUnique8")? / 100.0);
    let map_level = difficulty.require(&["MapLevel", "AreaLevel"])?;
    let map_mult: HashMap<i64, f64> = difficulty
        .rows()
        .map(|row| (row.int(map_level), 1.0 + row.int("DamagePercentIncrease") as f64 / 100.0))
        .collect();
    // PoB copies these from `Misc.lua`, which holds each value as `tostring` printed it.
    let base_damage: Vec<f64> = monster_stats
        .rows()
        .map(|row| crate::pob_export::lua::tostring(row.float("Damage") as f64).parse().unwrap_or(0.0))
        .collect();
    let base_damage_at = |level: f64| -> f64 {
        if level.fract() == 0.0 && level >= 1.0 {
            base_damage.get(level as usize - 1).copied().unwrap_or(1.0)
        } else {
            1.0
        }
    };
    let mut levels_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for row in per_level.rows() {
        for effect in row.list_keys(level_effects) {
            levels_of.entry(effect).or_default().push(row.index);
        }
    }
    let load = |id: &str| -> Option<EffectLevels<'_>> {
        let effect = effects.by_id(id)?;
        Some(EffectLevels {
            effect,
            stat_set: stat_sets.by_id(id),
            levels: levels_of.get(&effect.index).into_iter().flatten().filter_map(|&i| per_level.row(i)).collect(),
        })
    };

    let variety_mods = varieties.require(&["Mods", "ModsKeys"])?;
    let mut skills = Table::new();
    let mut list = Table::list([Table::new().with("val", "None").with("label", "None")]);
    let mut boss: Option<Boss> = None;
    for line in BOSS_SKILLS.lines().map(str::trim) {
        if let Some(args) = line.strip_prefix("#boss ") {
            let words: Vec<&str> = args.split(' ').collect();
            boss = match words[..] {
                [name, .., earlier, map] if words.len() >= 4 => {
                    varieties.by_id(&words[1..words.len() - 2].join(" ")).map(|variety| Boss {
                        name: name.to_string(),
                        earlier_uber: earlier == "true",
                        map_boss: map == "true",
                        unique: ctx.rr.deref_list_ids(variety, variety_mods).iter().any(|m| m == "MonsterMapBoss"),
                    })
                }
                _ => None,
            };
            continue;
        }
        let Some(args) = line.strip_prefix("#skill ") else { continue };
        let Some(boss) = boss.as_ref() else { continue };
        let mut words = args.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty());
        let (Some(display), Some(effect_id)) = (words.next(), words.next()) else { continue };
        let display = match display {
            "MemoryGame" => "Memory Game",
            "GroundDegen" => "Ground Degen",
            other => other,
        };
        let full_name = format!("{} {}", boss.name, display);
        list.push(Table::new().with("val", full_name.as_str()).with("label", full_name.as_str()));
        let Some(skill) = load(effect_id) else { continue };
        let param = |key: &str| directive_param(args, key);
        let index = param("skillIndex").map_or(Some(1), |v| v.parse::<usize>().ok());
        let uber_index = match param("skillIndexUber") {
            Some(v) => v.parse::<usize>().ok(),
            None => index.map(|i| i + 1),
        };
        let second_id = param("GrantedEffectId2");
        let second = second_id.and_then(load);
        let extra_damage = param("ExtraDamageMult").and_then(|v| v.parse::<f64>().ok()).map_or(1.0, |v| v / 100.0);
        let stages = param("stages").and_then(|v| v.parse::<f64>().ok());
        let speed_mult = param("speedMult").and_then(|v| v.parse::<f64>().ok());

        let damage_type = boss_damage_type(ctx, &skill);
        let rarity = match (boss.unique, damage_type.as_str()) {
            (true, "Melee" | "Projectile") => Some(unique_attack),
            (true, _) => Some(unique),
            (false, _) => None,
        };

        let mut extra = [1.0, 1.0];
        for (slot, i) in [index, uber_index].into_iter().map_while(|i| i).enumerate() {
            let Some(level) = level_at(&skill.levels, Some(i)) else { continue };
            if let Some(v) = level_stat(ctx, level, "AdditionalStats", "AdditionalStatsValues", "active_skill_damage_+%_final") {
                extra[slot] = 1.0 + v / 100.0;
            }
        }
        if let Some(stages) = stages {
            let per_stack = skill
                .stat_set
                .and_then(|set| set_constant(ctx, set, "charged_blast_spell_damage_+%_final_per_stack"))
                .map_or(1.0, |v| v / 100.0);
            extra = [extra[0] * (1.0 + per_stack * stages), extra[1] * (1.0 + per_stack * stages)];
        }

        let damage_id = match second_id {
            Some(id) if !HAND_DAMAGE_SKILLS.contains(&effect_id) && HAND_DAMAGE_SKILLS.contains(&id) => id,
            _ => effect_id,
        };
        let mut multipliers: Option<Table> = None;
        let mut uber_mult: Option<f64> = None;
        if HAND_DAMAGE_SKILLS.contains(&damage_id) {
            let monster_level = 84i64;
            let ratio = |a: i64, b: i64| Some(map_mult.get(&a)? / map_mult.get(&b)?);
            if extra[0] != extra[1] {
                let scale = if boss.map_boss { ratio(monster_level + 1, monster_level) } else { Some(1.0) };
                uber_mult = scale.map(|s| 100.0 * extra[1] / extra[0] * s);
            } else if damage_id != HAND_UBER_SKILL && boss.map_boss {
                uber_mult = ratio(monster_level + 1, monster_level).map(|r| 100.0 * r);
            }
        } else {
            let mut bases: HashMap<String, f64> = HashMap::new();
            for (slot, i) in [index, uber_index].into_iter().map_while(|i| i).enumerate() {
                let Some(level) = level_at(&skill.levels, Some(i)) else { continue };
                let floats = ctx.rr.deref_list_ids(level, "FloatStats");
                let values: Vec<f64> = level.list_int("BaseResolvedValues").into_iter().map(|v| v as f64).collect();
                for (stat, value) in floats.iter().zip(&values) {
                    for kind in DAMAGE_TYPES {
                        let lower = kind.to_ascii_lowercase();
                        if *stat == format!("spell_minimum_base_{}_damage", lower) {
                            bases.insert(format!("min{}{}", kind, slot + 1), 1.0 + value);
                        } else if *stat == format!("spell_maximum_base_{}_damage", lower) {
                            bases.insert(format!("max{}{}", kind, slot + 1), 1.0 + value);
                        }
                    }
                }
                if damage_type != "DamageOverTime" {
                    continue;
                }
                for (stat, value) in floats.iter().zip(&values) {
                    for kind in DAMAGE_TYPES {
                        if *stat == format!("base_{}_damage_to_deal_per_minute", kind.to_ascii_lowercase()) {
                            bases.insert(format!("min{}{}", kind, slot + 1), 1.0 + value / 60.0);
                            bases.insert(format!("max{}{}", kind, slot + 1), 1.0 + value / 60.0);
                        }
                    }
                }
            }
            let Some(first) = level_at(&skill.levels, index) else { continue };
            let monster_level = first.float("PlayerLevelReq") as f64;
            let map_at = |level: f64| if level.fract() == 0.0 { map_mult.get(&(level as i64)).copied() } else { None };
            let scale = if boss.map_boss { map_at(monster_level).unwrap_or(1.0) } else { 1.0 };
            let damage = extra_damage * extra[0] * rarity.unwrap_or(1.0) * scale / base_damage_at(monster_level);
            let mut table = Table::new();
            let (mut normal, mut uber) = (0.0, 0.0);
            for kind in DAMAGE_TYPES {
                let (min, max) = (bases.get(&format!("min{}1", kind)), bases.get(&format!("max{}1", kind)));
                if min.is_none() && max.is_none() {
                    continue;
                }
                let (low, high) = (damage * min.copied().unwrap_or(0.0), damage * max.copied().unwrap_or(0.0));
                table.set(kind, Table::list([low, (high - low) / 100.0]));
                normal += min.copied().unwrap_or(0.0);
                uber += bases.get(&format!("min{}2", kind)).copied().unwrap_or(0.0);
            }
            multipliers = Some(table);
            if let Some(uber_level) = level_at(&skill.levels, uber_index) {
                let uber_level = uber_level.float("PlayerLevelReq") as f64;
                let scale = match boss.map_boss {
                    true => map_at(uber_level).zip(map_at(monster_level)).map_or(f64::NAN, |(a, b)| a / b),
                    false => 1.0,
                };
                let mult = (uber / base_damage_at(uber_level)) / (normal / base_damage_at(monster_level)) * scale;
                if mult > 1.15 || mult < 0.85 {
                    uber_mult = Some((mult * 100.0).ceil());
                }
            }
        }

        let mut entry = Table::new().with("DamageType", damage_type.as_str());
        entry.set_opt("DamageMultipliers", multipliers);
        entry.set_opt("UberDamageMultiplier", uber_mult.map(|m| m / 100.0));
        set_penetrations(ctx, &skill, second.as_ref(), &mut entry);
        let speed_levels: Vec<f64> = skill
            .levels
            .iter()
            .take(2)
            .map(|level| {
                let values = level.list_int("AdditionalStatsValues");
                ctx.rr
                    .deref_list_ids(*level, "AdditionalStats")
                    .iter()
                    .position(|id| id == "active_skill_attack_speed_+%_final" || id == "active_skill_cast_speed_+%_final")
                    .and_then(|at| values.get(at))
                    .map_or(0.0, |&v| 100.0 + v as f64)
            })
            .collect();
        let (speed, uber_speed) = boss_speed(skill.effect.int("CastTime") as f64, None, &speed_levels, speed_mult, stages);
        entry.set_opt("speed", speed.filter(|s| *s != 700.0));
        entry.set_opt("UberSpeed", uber_speed.filter(|s| *s != 700.0));
        let crit = skill.levels.first().map(|level| (level.int(crit_column(level.table)) as f64 / 100.0).ceil()).unwrap_or(5.0);
        if crit != 5.0 {
            entry.set("critChance", crit);
        }
        if boss.earlier_uber {
            entry.set("earlierUber", true);
        }
        entry.set_opt("additionalStats", boss_additional_stats(ctx, &skill));
        skills.set(full_name, entry);
    }
    Ok(Table::new().with("bossSkills", skills).with("bossSkillsList", list))
}

/// PoB's spec calls the first crit column of `GrantedEffectStatSetsPerLevel`
/// `AttackCritChance`; dat-schema calls it `SpellCritChance`.
fn crit_column(table: &LoadedTable) -> &'static str {
    table.pick(&["SpellCritChance", "AttackCritChance"]).unwrap_or("AttackCritChance")
}

/// `statsPerLevel[i]`, 1-based.
fn level_at<'t>(levels: &[Row<'t>], i: Option<usize>) -> Option<Row<'t>> {
    levels.get(i?.checked_sub(1)?).copied()
}

/// `args:match("<key> = (%w+),")`.
fn directive_param<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    let at = args.find(&format!("{} = ", key))? + key.len() + 3;
    let rest = &args[at..];
    let len = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
    (len > 0 && rest[len..].starts_with(',')).then(|| &rest[..len])
}

/// The value a level row gives one stat in a paired id/value list.
fn level_stat(ctx: &Ctx, level: Row<'_>, ids: &str, values: &str, stat: &str) -> Option<f64> {
    let at = ctx.rr.deref_list_ids(level, ids).iter().position(|id| id == stat)?;
    level.list_int(values).get(at).map(|&v| v as f64)
}

/// A stat set's constant value for one stat.
fn set_constant(ctx: &Ctx, set: Row<'_>, stat: &str) -> Option<f64> {
    level_stat(ctx, set, "ConstantStats", "ConstantStatsValues", stat)
}

/// `getStat(state, "DamageType")`.
fn boss_damage_type(ctx: &Ctx, skill: &EffectLevels) -> String {
    let mut kind = "Untyped";
    if skill.stat_set.is_some_and(|set| ctx.rr.deref_list_ids(set, "ImplicitStats").iter().any(|s| s == "base_is_projectile")) {
        kind = "Projectile";
    }
    let active = ctx.rr.deref(skill.effect, "ActiveSkill");
    let (types, flags) = match &active {
        Some(a) => {
            let types_col = a.table.pick(&["ActiveSkillTypes", "SkillTypes"]).unwrap_or("ActiveSkillTypes");
            (ctx.rr.deref_list_ids(a.row(), types_col), ctx.rr.deref_list_ids(a.row(), "StatContextFlags"))
        }
        None => (Vec::new(), Vec::new()),
    };
    for t in &types {
        kind = match t.as_str() {
            "Attack" if kind == "Projectile" => "Projectile",
            "Attack" => "Melee",
            "Spell" if kind == "Projectile" => "SpellProjectile",
            "Spell" => "Spell",
            "Projectile" if kind == "Spell" => "SpellProjectile",
            "Projectile" => "Projectile",
            _ => kind,
        };
    }
    let mut hit = false;
    for f in &flags {
        kind = match f.as_str() {
            "AttackHit" => {
                hit = true;
                if kind == "Projectile" { "Projectile" } else { "Melee" }
            }
            "SpellHit" => {
                hit = true;
                if kind == "Projectile" { "SpellProjectile" } else { "Spell" }
            }
            "Projectile" if kind == "Spell" => "SpellProjectile",
            "Projectile" => "Projectile",
            _ => kind,
        };
    }
    if !hit && flags.iter().any(|f| f == "DamageOverTime") {
        return "DamageOverTime".into();
    }
    kind.into()
}

/// `getStat(state, "Penetration")` and the two tables it leads to.
fn set_penetrations(ctx: &Ctx, skill: &EffectLevels, second: Option<&EffectLevels>, entry: &mut Table) {
    const ELEMENTS: [&str; 3] = ["Lightning", "Cold", "Fire"];
    let mut base: HashMap<&str, f64> = HashMap::new();
    let mut uber: HashMap<&str, f64> = HashMap::new();
    for levels in [Some(skill), second].into_iter().flatten().map(|s| &s.levels) {
        for (i, level) in levels.iter().enumerate() {
            for element in ELEMENTS {
                let stat = format!("base_reduce_enemy_{}_resistance_%", element.to_ascii_lowercase());
                let values = level.list_int("AdditionalStatsValues");
                for (at, id) in ctx.rr.deref_list_ids(*level, "AdditionalStats").iter().enumerate() {
                    if *id == stat {
                        let value = values.get(at).map_or(0.0, |&v| v as f64);
                        if i == 0 { base.insert(element, value) } else { uber.insert(element, value) };
                    }
                }
            }
        }
    }
    // A base of 0 with an uber value becomes "", which counts as set.
    let pen = |element: &str| -> Option<Lua> {
        let b = base.get(element).copied().unwrap_or(0.0);
        let u = uber.get(element).copied().unwrap_or(0.0);
        match (b, u) {
            (b, _) if b != 0.0 => Some(Lua::Num(b)),
            (_, u) if u != 0.0 => Some(Lua::Str(String::new())),
            _ => None,
        }
    };
    let names = [("Lightning", "LightningPen"), ("Cold", "ColdPen"), ("Fire", "FirePen")];
    if names.iter().all(|(e, _)| pen(e).is_none()) {
        return;
    }
    let mut table = Table::new();
    for (element, name) in names {
        table.set_opt(name, pen(element));
    }
    entry.set("DamagePenetrations", table);
    let mut uber_table = Table::new();
    for (element, name) in names {
        uber_table.set_opt(name, uber.get(element).copied().filter(|v| *v != 0.0));
    }
    if !uber_table.is_empty() {
        entry.set("UberDamagePenetrations", uber_table);
    }
}

/// `getStat(state, "Speed")`: the cast time in ms, scaled by PoB's speed
/// tuning and the stage count, then by the first two levels' final speed
/// stats (`speed_levels`, 0 when a level has none).
fn boss_speed(
    cast_time: f64,
    uber_cast_time: Option<f64>,
    speed_levels: &[f64],
    speed_mult: Option<f64>,
    stages: Option<f64>,
) -> (Option<f64>, Option<f64>) {
    let mut speed = cast_time;
    let mut uber = uber_cast_time;
    if let Some(mult) = speed_mult {
        speed = speed * mult / 10000.0;
        uber = uber.map(|u| u * mult / 10000.0);
    }
    if let Some(stages) = stages {
        speed *= stages;
        uber = uber.map(|u| u * stages);
    }
    let first = speed_levels.first().copied().unwrap_or(0.0);
    let second = speed_levels.get(1).copied().unwrap_or(0.0);
    if first != 0.0 {
        if first != second {
            return (Some((speed / first * 100.0).ceil()), Some((speed / second * 100.0).ceil()));
        }
        speed = speed / first * 100.0;
        // PoB divides a missing uber cast time here and fails; no boss reaches it.
        uber = uber.map(|u| u / first * 100.0);
    }
    (Some(speed.ceil()), uber.map(f64::ceil))
}

/// `getStat(state, "AdditionalStats")`: flags and values the first two
/// levels, and the stat set, add to the skill.
fn boss_additional_stats(ctx: &Ctx, skill: &EffectLevels) -> Option<Table> {
    let (mut base, mut uber) = (Table::new(), Table::new());
    let flag = || Lua::from("flag");
    for (i, level) in skill.levels.iter().take(2).enumerate() {
        let target = if i == 0 { &mut base } else { &mut uber };
        for (stat, key) in [("global_reduce_enemy_block_%", "reduceEnemyBlock"), ("reduce_enemy_dodge_%", "reduceEnemyDodge")] {
            if let Some(v) = level_stat(ctx, *level, "AdditionalStats", "AdditionalStatsValues", stat) {
                target.set(key, v);
            }
        }
        let flags_col = level.table.pick(&["AdditionalFlags", "AdditionalBooleanStats"]).unwrap_or("AdditionalFlags");
        for id in ctx.rr.deref_list_ids(*level, flags_col) {
            match id.as_str() {
                "global_always_hit" => {
                    target.set("CannotBeEvaded", flag());
                }
                "cannot_be_blocked_or_dodged_or_suppressed" => {
                    for key in ["CannotBeBlocked", "CannotBeDodged", "CannotBeSuppressed"] {
                        target.set(key, flag());
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(set) = skill.stat_set {
        if ctx.rr.deref_list_ids(set, "ImplicitStats").iter().any(|s| s == "cannot_be_blocked_or_dodged_or_suppressed") {
            for target in [&mut base, &mut uber] {
                for key in ["CannotBeBlocked", "CannotBeDodged", "CannotBeSuppressed"] {
                    target.set(key, flag());
                }
            }
        }
        let values = set.list_int("ConstantStatsValues");
        for (at, id) in ctx.rr.deref_list_ids(set, "ConstantStats").iter().enumerate() {
            let key = match id.as_str() {
                "skill_physical_damage_%_to_convert_to_lightning" => "PhysicalDamageSkillConvertToLightning",
                "skill_physical_damage_%_to_convert_to_cold" => "PhysicalDamageSkillConvertToCold",
                "skill_physical_damage_%_to_convert_to_fire" => "PhysicalDamageSkillConvertToFire",
                "skill_physical_damage_%_to_convert_to_chaos" => "PhysicalDamageSkillConvertToChaos",
                _ => continue,
            };
            let value = values.get(at).map_or(0.0, |&v| v as f64);
            base.set(key, value);
            uber.set(key, value);
        }
    }
    if base.is_empty() && uber.is_empty() {
        return None;
    }
    let mut out = Table::new();
    if !base.is_empty() {
        out.set("base", base);
    }
    if !uber.is_empty() {
        out.set("uber", uber);
    }
    Some(out)
}

/// `WorldAreas.lua` (PoE 2): every named area with its act, level and tags,
/// the spectres met there and its bosses.
pub fn world_areas(ctx: &Ctx) -> Result<(), String> {
    let monsters = Monsters::new(ctx)?;
    let spectres: HashSet<String> = spectre_ids(ctx, &monsters)?
        .iter()
        .filter_map(|id| monsters.varieties.by_id(id))
        .map(|row| row.string("Name"))
        .collect();
    let entries = ctx.table("MonsterPackEntries")?;
    let packs = ctx.table("MonsterPacks")?;
    let areas = ctx.table("WorldAreas")?;
    let maps = ctx.table("EndgameMaps")?;
    let pack_areas = packs.require(&["WorldAreas", "WorldAreasKeys"])?;
    let additional = packs.pick(&["AdditionalMonsters"]);
    let bosses = packs.require(&["BossMonsters", "BossMonster_MonsterVarietiesKeys"])?;
    let map_area = maps.require(&["WorldArea", "Id"])?;
    let map_packs = maps.require(&["MonsterPacks", "NativePacks"])?;
    let area_tags = areas.require(&["Tags", "TagsKeys"])?;
    let area_bosses = areas.require(&["Bosses_MonsterVarietiesKeys", "Bosses"])?;
    let description = areas.column_or_after(&["Description"], "QuestFlags", 2);

    let mut pack_monsters: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries.rows() {
        let (Some(pack), Some(variety)) = (ctx.rr.deref(entry, "MonsterPacksKey"), ctx.rr.deref(entry, "MonsterVarietiesKey")) else {
            continue;
        };
        let name = variety.row().string("Name");
        if !name.is_empty() {
            pack_monsters.entry(pack.id()).or_default().push(name);
        }
    }
    for pack in packs.rows() {
        let list = pack_monsters.entry(pack.id().to_string()).or_default();
        let mut seen: HashSet<String> = list.iter().cloned().collect();
        for col in additional.iter().copied().chain([bosses]) {
            for monster in ctx.rr.deref_list(pack, col) {
                let name = monster.row().string("Name");
                if !name.is_empty() && seen.insert(name.clone()) {
                    list.push(name);
                }
            }
        }
    }

    let mut area_monsters: HashMap<String, (Vec<String>, HashSet<String>)> = HashMap::new();
    let mut add = |area: String, pack: &str| {
        let (list, seen) = area_monsters.entry(area).or_default();
        for name in pack_monsters.get(pack).into_iter().flatten() {
            if seen.insert(name.clone()) {
                list.push(name.clone());
            }
        }
    };
    for pack in packs.rows() {
        for area in ctx.rr.deref_list(pack, pack_areas) {
            add(area.id(), pack.id());
        }
    }
    let mut descriptions: HashMap<String, String> = HashMap::new();
    for map in maps.rows() {
        let Some(area) = ctx.rr.deref_id(map, map_area) else { continue };
        for pack in ctx.rr.deref_list_ids(map, map_packs) {
            add(area.clone(), &pack);
        }
        let flavour = map.str("FlavourText");
        if flavour.is_empty() {
            continue;
        }
        let text = match area.ends_with("_Claimable") {
            true => first_sentence(flavour).map(|s| s.trim_end_matches(is_lua_space).to_string()),
            false => None,
        };
        descriptions.insert(area, text.unwrap_or_else(|| flavour.to_string()));
    }

    let mut out = Table::new();
    for area in areas.rows() {
        let (id, name) = (area.id(), area.str("Name"));
        if name == "NULL" || name.contains("DNT") {
            continue;
        }
        if id.contains("Design") || id.contains("Programming") || id == "BlackTest" || id == "G_Endgame_Town" {
            continue;
        }
        let tags = ctx.rr.deref_list_ids(area, area_tags);
        let is_map = tags.iter().any(|t| t == "map");
        let act = area.int("Act");
        let suffix = if is_map {
            " (Map)".to_string()
        } else if let Some(floor) = sanctum_floor(id) {
            format!(" (Floor {})", floor)
        } else if act != 10 {
            format!(" (Act {})", act)
        } else {
            String::new()
        };
        let mut entry = Table::new()
            .with("name", lua_literal(&format!("{}{}", name, suffix)))
            .with("baseName", lua_literal(name));
        let own = description.and_then(|c| match area.cell(c) {
            Some(crate::dat::reader::DatValue::String(s)) => Some(s.as_str()),
            _ => None,
        });
        if let Some(text) = descriptions.get(id).map(String::as_str).or(own).filter(|s| !s.is_empty()) {
            entry.set("description", lua_literal(text));
        }
        entry.set("tags", Table::list(tags.iter().map(|t| lua_literal(t))));
        entry.set("act", act);
        entry.set("level", area.int("AreaLevel"));
        entry.set("isMap", is_map);
        entry.set("isHideout", area.bool("IsHideout"));
        let mut met: Vec<&String> = area_monsters.get(id).map(|(list, _)| list.iter().collect()).unwrap_or_default();
        met.sort();
        met.dedup();
        entry.set("monsterVarieties", Table::list(met.into_iter().filter(|n| spectres.contains(*n)).map(|n| lua_literal(n))));
        let boss_rows = ctx.rr.deref_list(area, area_bosses);
        if !boss_rows.is_empty() {
            let mut names: Vec<String> = Vec::new();
            for boss in &boss_rows {
                let boss_id = boss.row().id();
                let Some(variety) = monsters.varieties.by_id(boss_id).filter(|_| !boss_id.is_empty()) else { continue };
                let boss_name = variety.string("Name");
                if !boss_name.is_empty() && !boss_name.contains("DNT") && !names.contains(&boss_name) {
                    names.push(boss_name);
                }
            }
            entry.set("bossVarieties", Table::list(names.iter().map(|n| lua_literal(n))));
        }
        out.set(lua_literal(id), entry);
    }
    write(ctx, "WorldAreas", out)
}

/// `text:match("([^%.%!%?]+[%.%!%?])")`: the first run of text up to and
/// including a full stop, exclamation or question mark.
fn first_sentence(text: &str) -> Option<&str> {
    let is_end = |c: char| matches!(c, '.' | '!' | '?');
    let start = text.find(|c: char| !is_end(c))?;
    let end = text[start..].find(is_end)? + start;
    Some(&text[start..=end])
}

/// `Misc.lua` and `CurrencyNames.lua`, plus PoE 2's `CharacterMeleeSkills.lua`.
pub fn misc(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let mut data = Table::new();

    let stats = ctx.table("DefaultMonsterStats")?;
    let column = |names: &[&str]| -> Result<String, String> { stats.require(names).map(str::to_string) };
    let mut tables: Vec<(&str, String)> = vec![
        ("monsterEvasionTable", column(&["Evasion"])?),
        ("monsterAccuracyTable", column(&["Accuracy"])?),
        ("monsterLifeTable", column(&["MonsterLife"])?),
    ];
    if game == Game::Poe1 {
        tables.push(("monsterLifeTable2", column(&["AltLife1"])?));
        tables.push(("monsterLifeTable3", column(&["AltLife2"])?));
    }
    tables.push(("monsterAllyLifeTable", column(&["MinionLife", "AllyLife"])?));
    tables.push(("monsterDamageTable", column(&["Damage"])?));
    tables.push(("monsterAllyDamageTable", column(&["MinionDamage"])?));
    if game == Game::Poe2 {
        tables.push(("monsterArmourTable", column(&["Armour"])?));
    }
    tables.push(("monsterAilmentThresholdTable", column(&["AilmentThreshold"])?));
    match game {
        Game::Poe2 => tables.push(("monsterPoiseThresholdTable", column(&["PoiseThreshold"])?)),
        Game::Poe1 => tables.push(("monsterPhysConversionMultiTable", column(&["MonsterPhysConversionMulti"])?)),
    }
    for (name, col) in &tables {
        data.set(*name, Table::list(stats.rows().map(|row| cell_number(row, col))));
    }

    let constants = ctx.table("GameConstants")?;
    if game == Game::Poe1 {
        // PoE 1's armour column is wrong, so PoB builds the table from the
        // armour improvement constant instead.
        let improvement = constants
            .by_id("MonsterDamageReductionImprovement")
            .map(|row| row.int("Value") as f64 / row.int("Divisor") as f64)
            .ok_or("GameConstants has no MonsterDamageReductionImprovement")?;
        let armour = (1..=100).map(|i| {
            let i = i as f64;
            ((10.0 + 2.0 * i) * (1.0 + improvement / 100.0).powf(i)).floor()
        });
        data.set("monsterArmourTable", Table::list(armour));
    }

    if game == Game::Poe2 {
        let levels = ctx.table("MinionGemLevelScaling")?;
        let minion = levels.require(&["MinionLevel"])?;
        data.set("minionLevelTable", Table::list(levels.rows().map(|row| row.int(minion))));
    }

    let mut game_constants = Table::new();
    for row in constants.rows() {
        let value = row.int("Value") as f64 / row.int("Divisor") as f64;
        // `inf` and `nan` read back as an unset global.
        if value.is_finite() {
            game_constants.set(lua_literal(row.id()), value);
        }
    }
    data.set("gameConstants", game_constants);

    let character = read_text(ctx, "Metadata/Characters/Character.ot").ok_or("Metadata/Characters/Character.ot is missing")?;
    data.set("characterConstants", object_block_values(&character, &["Stats", "Pathfinding"]));
    let monster = read_text(ctx, "Metadata/Monsters/Monster.ot").ok_or("Metadata/Monsters/Monster.ot is missing")?;
    data.set("monsterConstants", object_block_values(&monster, &["Stats"]));

    if game == Game::Poe2 {
        let intrinsic = ctx.table("PlayerMinionIntrinsicStats")?;
        let stat = intrinsic.require(&["Stat", "Id"])?;
        let mut out = Table::new();
        for row in intrinsic.rows() {
            if let Some(id) = ctx.rr.deref_id(row, stat) {
                out.set(lua_literal(&id), row.int("Value"));
            }
        }
        data.set("playerMinionIntrinsicStats", out);
    }

    let totems = ctx.table("SkillTotemVariations")?;
    let totem = totems.require(&["SkillTotemsKey", "SkillTotem"])?;
    let variety = totems.require(&["MonsterVarietiesKey", "MonsterVariety"])?;
    let mut totem_life = Table::new();
    for row in totems.rows() {
        let key = row.int(totem);
        if totem_life.contains(key) {
            continue;
        }
        let mult = match ctx.rr.deref(row, variety) {
            Some(v) => v.row().int("LifeMultiplier") as f64 / 100.0,
            None if game == Game::Poe2 => 1.0,
            None => return Err(format!("SkillTotemVariations row {} names no monster", row.index)),
        };
        totem_life.set(key, mult);
    }
    data.set("totemLifeMult", totem_life);

    let varieties = ctx.table("MonsterVarieties")?;
    let mods = varieties.require(&["Mods", "ModsKeys"])?;
    let mut raisable = Table::new();
    for row in varieties.rows() {
        let name = lua_literal(row.str("Name"));
        if raisable.contains(name.as_str()) {
            continue;
        }
        if ctx.rr.deref_list_ids(row, mods).iter().any(|id| id == "MonsterNecromancerRaisable") {
            raisable.set(name, row.int("LifeMultiplier") as f64 / 100.0);
        }
    }
    data.set("monsterVarietyLifeMult", raisable);

    let packs = ctx.table("MonsterMapDifficulty")?;
    let level = packs.require(&["MapLevel", "AreaLevel"])?;
    let mut map_life = Table::new();
    for row in packs.rows() {
        map_life.set(row.int(level), 1.0 + row.int("LifePercentIncrease") as f64 / 100.0);
    }
    data.set("mapLevelLifeMult", map_life);

    let bosses = ctx.table("MonsterMapBossDifficulty")?;
    let level = bosses.require(&["MapLevel", "AreaLevel"])?;
    let (mut boss_life, mut boss_ailment) = (Table::new(), Table::new());
    for row in bosses.rows() {
        boss_life.set(row.int(level), 1.0 + row.int("BossLifePercentIncrease") as f64 / 100.0);
        boss_ailment.set(row.int(level), (100.0 + row.int("BossAilmentPercentDecrease") as f64) / 100.0);
    }
    data.set("mapLevelBossLifeMult", boss_life);
    data.set("mapLevelBossAilmentMult", boss_ailment);

    match game {
        Game::Poe2 => {
            let flat = ctx.table("FlatPhysicalDamageValues")?;
            let level = flat.require(&["GemLevel", "Level"])?;
            let mut palm = Table::new();
            for row in flat.rows() {
                palm.set(row.int(level), Table::list([row.int("MinPhys"), row.int("MaxPhys")]));
            }
            data.set("hollowPalmAddedPhys", palm);
            let prices = ctx.table("GoldRespecPrices")?;
            let cost = prices.require(&["Cost"])?;
            data.set("goldRespecPrices", Table::list(prices.rows().map(|row| row.int(cost))));
        }
        Game::Poe1 => {
            let village = ctx.table("VillageBalancePerLevelShared")?;
            let cost = village.require(&["GoldRespecCost", "GoldRespec"])?;
            data.set("goldRespecPrices", Table::list(village.rows().map(|row| row.int(cost))));
        }
    }
    write(ctx, "Misc", data)?;

    write(ctx, "CurrencyNames", currency_names(ctx)?)?;
    if game == Game::Poe2 {
        write(ctx, "CharacterMeleeSkills", character_melee_skills(ctx)?)?;
    }
    Ok(())
}

/// A number column as Lua reads it back after `..` wrote it with `tostring`.
fn cell_number(row: Row<'_>, col: &str) -> Lua {
    match row.get(col) {
        Some(crate::dat::reader::DatValue::Float(f)) => Lua::from(*f),
        _ => Lua::from(row.int(col)),
    }
}

/// `CurrencyNames.lua`: base item id to name for every stackable currency.
/// PoE 2 takes every such base; PoE 1 only those on the currency exchange.
fn currency_names(ctx: &Ctx) -> Result<Table, String> {
    let bases = ctx.table("BaseItemTypes")?;
    let class = bases.require(&["ItemClass", "ItemClassesKey"])?;
    let is_currency = |row: Row<'_>| ctx.rr.deref_id(row, class).is_some_and(|id| id == "StackableCurrency");
    let mut out = Table::new();
    let mut add = |row: Row<'_>| {
        out.set(saved_string(row.id()), saved_string(row.str("Name")));
    };
    match game(ctx) {
        Game::Poe2 => {
            for row in bases.rows().filter(|r| is_currency(*r) && !r.str("Name").is_empty()) {
                add(row);
            }
        }
        Game::Poe1 => {
            let exchange = ctx.table("CurrencyExchange")?;
            let item = exchange.require(&["Item", "BaseItemType"])?;
            for entry in exchange.rows() {
                let Some(base) = ctx.rr.deref(entry, item) else { continue };
                let row = base.row();
                if is_currency(row) && !row.str("Name").is_empty() && !row.str("Name").contains("DNT") {
                    add(row);
                }
            }
        }
    }
    Ok(out)
}

/// `CharacterMeleeSkills.lua` (PoE 2): the default attack gems for each pair
/// of main-hand and off-hand item classes.
fn character_melee_skills(ctx: &Ctx) -> Result<Table, String> {
    let table = ctx.table("CharacterMeleeSkills")?;
    let main = table.require(&["MainHand", "MainHandItem"])?;
    let off = table.require(&["OffHand", "OffHandItem"])?;
    let gems = table.require(&["SkillGems", "SkillGem"])?;
    let class_of = |row: Row<'_>, col: &str| -> Option<String> {
        let wieldable = ctx.rr.deref(row, col)?;
        let class = wieldable.table.pick(&["ItemClass", "ItemClassesKey"])?;
        ctx.rr.deref_id(wieldable.row(), class)
    };
    let mut out = Table::new();
    for row in table.rows() {
        let (Some(main_hand), Some(off_hand)) = (class_of(row, main), class_of(row, off)) else {
            return Err(format!("CharacterMeleeSkills row {} names no item class", row.index));
        };
        let skill_gems: Vec<String> = ctx
            .rr
            .deref_list(row, gems)
            .iter()
            .filter_map(|gem| {
                let base = gem.table.pick(&["BaseItemType", "BaseItemTypesKey"])?;
                ctx.rr.deref_id(gem.row(), base)
            })
            .collect();
        out.table_mut(saved_string(&main_hand))
            .set(saved_string(&off_hand), Table::list(skill_gems.iter().map(|s| saved_string(s))));
    }
    Ok(out)
}

/// A string as `utils.saveTableToFile` leaves it: `%q` round-trips, but line
/// breaks become spaces first.
fn saved_string(s: &str) -> String {
    s.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// `miscdata.lua`'s reading of an object file: every `key = value` line in a
/// block whose header line starts with one of `blocks`, whitespace removed,
/// the value read back as the Lua source it is written out as. A header with
/// its braces on the same line leaves the block open until the next line
/// starting with `}`, as in PoB.
fn object_block_values(text: &str, blocks: &[&str]) -> Table {
    let mut out = Table::new();
    let mut in_block = false;
    for line in text.split(['\r', '\n']).filter(|l| !l.is_empty()) {
        if blocks.iter().any(|b| line.starts_with(b)) {
            in_block = true;
        } else if in_block && line.starts_with('}') {
            in_block = false;
        } else if in_block && line.contains('=') {
            let squashed: String = line.chars().filter(|c| !is_lua_space(*c)).collect();
            let Some((key, value)) = squashed.split_once('=') else { continue };
            if value.is_empty() {
                continue;
            }
            if let Some(value) = lua_value(value) {
                out.set(lua_literal(key), value);
            }
        }
    }
    out
}

/// Lua's `%s`.
fn is_lua_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
}

/// A Lua expression PoB copies verbatim: a number, a boolean or a quoted
/// string. Anything else reads as an unset global, so the key is left out.
fn lua_value(source: &str) -> Option<Lua> {
    match source {
        "true" => return Some(Lua::Bool(true)),
        "false" => return Some(Lua::Bool(false)),
        _ => {}
    }
    if source.len() >= 2 && source.starts_with('"') && source.ends_with('"') {
        return Some(Lua::Str(lua_literal(&source[1..source.len() - 1])));
    }
    lua_number(source).map(Lua::Num)
}

/// A Lua numeric literal, with any leading minus signs Lua applies as
/// operators.
fn lua_number(source: &str) -> Option<f64> {
    let digits = source.trim_start_matches('-');
    let negations = source.len() - digits.len();
    if negations > 1 {
        // `--` starts a comment.
        return None;
    }
    let value = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()? as f64
    } else if !digits.is_empty()
        && digits.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        && digits.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
    {
        digits.parse().ok()?
    } else {
        return None;
    };
    Some(if negations == 1 { -value } else { value })
}

/// `Costs.lua`: every cost type, then PoB's own entry for soul costs.
pub fn costs(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let table = ctx.table("CostTypes")?;
    let stat = table.require(&["Stat", "StatsKey"])?;
    let mut out = Table::new();
    for row in table.rows() {
        let resource = row.id();
        let mut resource_string = row.string("FormatText");
        if game == Game::Poe2 && resource.starts_with("Ward") {
            resource_string = resource_string.replace("Ward", "Runic Ward");
        }
        let entry = Table::new()
            .with("Resource", lua_literal(resource))
            .with_opt("Stat", ctx.rr.deref_id(row, stat).map(|id| lua_literal(&id)))
            .with("ResourceString", lua_literal(&resource_string))
            .with("Divisor", row.int("Divisor"));
        out.set(row.index + 1, entry);
    }
    let soul = Table::new()
        .with("Resource", "Soul")
        .with("Stat", " ")
        .with("ResourceString", "{0} Souls Per Use")
        .with("Divisor", 1);
    out.set(table.len() + 1, soul);
    write(ctx, "Costs", out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_blocks_read_like_miscdata() {
        let text = "version 3\r\nStats\r\n{\r\n\tlevel = 1\r\n\tpvp_+% = -15\r\n\tenable = true\r\n\t// note\r\n}\r\n\
                    Positioned\r\n{\r\n\tteam = 1\r\n}\r\nPathfinding\r\n{\r\n\tbase_speed = 37\r\n}\r\n";
        let t = object_block_values(text, &["Stats", "Pathfinding"]);
        assert_eq!(t.get("level"), Some(&Lua::Num(1.0)));
        assert_eq!(t.get("pvp_+%"), Some(&Lua::Num(-15.0)));
        assert_eq!(t.get("enable"), Some(&Lua::Bool(true)));
        assert_eq!(t.get("base_speed"), Some(&Lua::Num(37.0)));
        assert!(t.get("team").is_none());
        assert_eq!(lua_number("0.2"), Some(0.2));
        assert_eq!(lua_number("inf"), None);
        assert_eq!(lua_number("--5"), None);
        assert_eq!(lua_value("\"Medium\""), Some(Lua::Str("Medium".into())));
    }

    #[test]
    fn world_area_text_rules() {
        assert_eq!(first_sentence("A fortress of fallen wood. Claim it."), Some("A fortress of fallen wood."));
        assert_eq!(first_sentence("..Why? Because."), Some("Why?"));
        assert_eq!(first_sentence("No end"), None);
        assert_eq!(sanctum_floor("Sanctum_3_Boss"), Some("3"));
        assert_eq!(sanctum_floor("Sanctum_Foyer"), None);
        assert_eq!(saved_string("a\r\nb\nc"), "a b c");
    }

    #[test]
    fn boss_directives_read_like_bossdata() {
        let args = "GroundDegen SynthesisVenariusQuicksand, skillIndexUber = nil, SkillExtraDamageMult = 226,";
        assert_eq!(directive_param(args, "skillIndexUber"), Some("nil"));
        assert_eq!(directive_param(args, "ExtraDamageMult"), Some("226"));
        assert_eq!(directive_param(args, "skillIndex"), None);
        assert_eq!(directive_param("Flameblast X, stages = 10,", "stages"), Some("10"));
        // Shaper Slam: a speed tuning, then different final speeds per level.
        assert_eq!(boss_speed(4000.0, None, &[100.0, 200.0], Some(8775.0), None), (Some(3510.0), Some(1755.0)));
        assert_eq!(boss_speed(2500.0, None, &[0.0, 0.0], None, Some(10.0)), (Some(25000.0), None));
    }

    #[test]
    fn poe1_spectre_list_expands() {
        let ids = poe1_spectre_ids();
        assert_eq!(ids.len(), 268);
        assert!(ids.contains(&"Metadata/Monster/CageSpider/CageSpider2".to_string()));
        assert!(ids.contains(&"Metadata/Monsters/LeagueAzmeri/SpecialCorpses/Mannequin/MannequinHigh_".to_string()));
        let unique: HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }
}
