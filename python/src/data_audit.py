"""MTGA comprehensive data source audit.

Generated 2026-06-06 by decompiling game assemblies, parsing Player.log,
inspecting the card database, and scanning the file system.

Use alongside protocol_notes.py for a complete picture. This file contains
NO executable code — it is a pure reference document.

Sources:
    DLLs decompiled:  Core.dll, SharedClientCore.dll, Wizards.Arena.Models.dll,
                      Wizards.Arena.Enums.dll, Wizards.Arena.Models.Protobuf.dll,
                      Contracts.dll, HasbroGo.dll
    Game version:     2026.59.0.124 (GRP 124.1273149)
    Player.log:       ~29 MB, 94213 lines, from current session
    Card database:    Raw_CardDatabase_00e9974dcc77f26a29e34b35365fe7d0.mtga (226.89 MB)
    MTGA install:     C:\\Program Files (x86)\\Steam\\steamapps\\common\\MTGA


================================================================================
SECTION 1: COMPLETE CmdType ENUM
================================================================================

The protobuf-canonical enum lives in Wizards.Arena.Models.Network.CmdType
(Wizards.Arena.Models.Protobuf.dll). The legacy ECmdType in
Wizards.Unification.Models.FrontDoor is marked [Obsolete] but has the same
numeric values for shared entries.

The protobuf version is AUTHORITATIVE and contains entries not in the legacy enum
(e.g., Event_SetDeckV3=627, Tournament commands 2650-2656, Lobby/Gathering/
Challenge extensions, ExternalChat 4000-4003).

CmdType enum (protobuf, complete):
    Authenticate                        = 0
    StartHook                           = 1
    Scaling_Passthrough                  = 2
    TencentSecurity_AntiHackData         = 3
    Tencent_RmtCreateOrder               = 4
    Attach                              = 5
    GetFormats                          = 6
    ForceDetach                         = 7
    GetConflictingAccountsData           = 8

    FriendRequestNotification            = 100

    Grants_ListPendinGrants              = 300
    Grants_ClaimPendingGrants            = 301

    --- Deck commands ---
    Deck_GetDeck                        = 400
    Deck_GetDeckSummaries               = 401     (obsolete)
    Deck_UpsertDeck                     = 402     (obsolete)
    Deck_DeleteDeck                     = 403
    Deck_GetAllPreconDecks              = 404     (obsolete)
    Deck_GetPreconDeck                  = 405     (obsolete)
    Deck_UpsertDeckV2                   = 406
    Deck_GetDeckSummariesV2             = 407
    Deck_GetAllPreconDecksV2            = 408
    Deck_GetPreconDeckV2                = 409
    Deck_GetAllPreconDecksV3            = 410
    Deck_GetDeckSummariesV3             = 411
    Deck_UpsertDeckV3                   = 412

    --- Direct Challenge ---
    DC_IssueChallenge                   = 500
    DC_CancelChallenge                  = 501

    --- Card commands ---
    Card_GetCardSet                     = 550     NOT LOGGED
    Card_GetAllCards                    = 551     NOT LOGGED
    Card_RedeemWildCards                = 552     NOT LOGGED
    Card_CompleteVault                  = 553     NOT LOGGED

    --- Event commands ---
    Event_Join                          = 600
    Event_Drop                          = 601
    Event_SetDeck                       = 602     (obsolete)
    Event_EnterPairing                  = 603
    Event_GetActiveEvents               = 604
    Event_GetCourses                    = 605     (obsolete)
    Event_LeavePairing                  = 606
    Event_ClaimPrize                    = 607
    Event_GetMatchResultReport          = 608
    Event_Resign                        = 609
    Event_GetChoices                    = 610
    Event_SetChoice                     = 611
    Event_AiBotMatch                    = 612
    Event_GetActiveMatches              = 613
    Event_SetJumpStartPacket            = 614
    Event_JoinDraftQueue                = 615
    Event_LeaveDraftQueue               = 616
    Event_PlayerDraftReadyPlayer         = 617
    Event_PlayerDraftGetTablePacks       = 618
    Event_JoinDraft                     = 619
    Event_PlayerDraftMakePick            = 620
    Event_PlayerDraftConfirmCardPoolGrant = 621
    Event_SetDeckV2                     = 622
    Event_GetCoursesV2                  = 623
    Event_GetActiveEventsV2             = 624
    Event_PlayerDraftReserveCard         = 625
    Event_PlayerDraftClearReservedCard    = 626
    Event_SetDeckV3                     = 627     (protobuf only, not in legacy enum)

    --- Store / Mercantile ---
    Store_GetEntitlements               = 703
    Carousel_GetCarouselItems           = 704
    Merc_GetSkus                        = 705     (obsolete)
    Merc_GetListingsV2                  = 706     (obsolete)
    Merc_PurchaseV2                     = 707
    Merc_GetStoreStatusV2               = 708
    Merc_GetListingsV3                  = 709     (obsolete)
    Merc_GetListingsV4                  = 710
    Merc_GetSkusV2                      = 711
    Store_GetEntitlementsV2             = 712
    Merc_GetSkusV3                      = 713
    Merc_GetListingsV5                  = 714
    Merc_GetSkusAndListings             = 715

    --- Currency ---
    Currency_GetCurrencies              = 800     NOT LOGGED

    --- Booster ---
    Booster_OpenBoosters                = 900     NOT LOGGED (response comes as InventoryChanged push)
    Booster_GetOwnedBoosters            = 901     NOT LOGGED

    --- Quest ---
    Quest_GetQuests                     = 1000
    Quest_SwapQuest                     = 1001

    --- Rank ---
    Rank_GetCombinedRankInfo            = 1100
    Rank_EvaluatePayouts                = 1101
    Rank_GetSeasonAndRankDetails        = 1102
    Rank_GetSeasonHistory               = 1103
    Rank_GetSeasonAndRewardHistory      = 1104
    Rank_EvaluatePayoutsV2              = 1105

    --- Periodic Rewards ---
    PeriodicRewards_GetStatus           = 1200
    Renewal_GetCurrentRenewal           = 1201
    Renewal_RedeemRewards               = 1202

    --- Definitions ---
    GetVoucherDefinitions               = 1520
    GetSets                             = 1521

    --- Account ---
    Account_GetTencentScreenName        = 1600
    Account_SetTencentScreenName        = 1601
    Account_GetAvatar                   = 1602
    Account_SetAvatar                   = 1603

    --- Campaign Graph ---
    Graph_GetGraphDefinitions           = 1700
    Graph_GetGraphState                 = 1701
    Graph_Process                       = 1702
    Graph_ProcessV2                     = 1703
    Graph_SetFavoriteNodes              = 1704

    --- Bot Draft ---
    BotDraft_StartDraft                 = 1800
    BotDraft_DraftPick                  = 1801
    BotDraft_DraftStatus                = 1802

    --- Cosmetics ---
    Cosmetics_GetPlayerOwnedCosmetics   = 1900
    Cosmetics_GetPreferredCosmetics     = 1901
    Cosmetics_SetPreferredAvatar        = 1902
    Cosmetics_SetPreferredSleeve        = 1903
    Cosmetics_SetPreferredPet           = 1904
    Cosmetics_SetPreferredTitle         = 1905
    Cosmetics_PurchaseCosmetic          = 1906
    BotDraft_Debug_UnloadCache          = 1907
    Draft_CompleteDraft                 = 1908
    Cosmetics_SetPreferredEmotes        = 1909

    --- Misc ---
    GetPlayBladeQueueConfig             = 1910
    GetPlayerPreferences                = 1911
    SetPlayerPreferences                = 1912
    LogBusinessEvents                   = 1913
    LogBusinessEventsV2                 = 1914

    GetNetDeckFolders                   = 2200

    --- Player Inbox ---
    GetPlayerInbox                      = 2300
    MarkLetterRead                      = 2301
    ClaimAttachment                     = 2302

    GetDesignerMetadata                 = 2400
    StaticContent                       = 2500

    --- Preferred Printings ---
    GetAllPreferredPrintings            = 2600
    SetPreferredPrinting                = 2601
    RemovePreferredPrinting             = 2602

    --- Tournament ---
    TournamentCreate                    = 2650    (protobuf only)
    TournamentJoin                      = 2651    (protobuf only)
    TournamentDropPlayer                = 2652    (protobuf only)
    TournamentGetState                  = 2653    (protobuf only)
    TournamentPlayerReadyRound          = 2654    (protobuf only)
    TournamentReportMatchResult         = 2655    (protobuf only)
    TournamentPlayerReadyStart          = 2656    (protobuf only)

    GetAllPrizeWalls                    = 2700

    --- Lobby ---
    LobbyJoin                          = 2800
    LobbyCreate                        = 2801
    LobbySendMessage                    = 2802
    LobbyExit                          = 2803
    LobbyInvite                        = 2804    (protobuf only)
    LobbyClose                         = 2805    (protobuf only)
    LobbyReconnectAll                   = 2806    (protobuf only)
    LobbyKick                          = 2807    (protobuf only)

    --- Gathering ---
    GatheringJoin                       = 2900
    GatheringCreate                     = 2901
    GatheringSendMessage                = 2902
    GatheringExit                       = 2903
    GatheringInvite                     = 2904    (protobuf only)
    GatheringClose                      = 2905    (protobuf only)
    GatheringReconnectAll               = 2906    (protobuf only)
    GatheringKick                       = 2907    (protobuf only)
    GatheringUpdateMetadata             = 2908    (protobuf only)
    GatheringAssignRole                 = 2909    (protobuf only)
    GatheringRemoveRole                 = 2910    (protobuf only)
    GatheringGenerateInviteCode         = 2911    (protobuf only)
    GatheringClearInviteCode            = 2912    (protobuf only)
    GatheringJoinByInviteCode           = 2913    (protobuf only)
    GatheringCreateV2                   = 2914    (protobuf only)

    --- Challenge ---
    ChallengeJoin                       = 3000
    ChallengeCreate                     = 3001
    ChallengeSendMessage                = 3002
    ChallengeExit                       = 3003
    ChallengeInvite                     = 3004    (protobuf only)
    ChallengeClose                      = 3005    (protobuf only)
    ChallengeReconnectAll               = 3006    (protobuf only)
    ChallengeKick                       = 3007    (protobuf only)
    ChallengeReady                      = 3008    (protobuf only)
    ChallengeUnready                    = 3009    (protobuf only)
    ChallengeSetSettings                = 3010    (protobuf only)
    ChallengeIssue                      = 3011    (protobuf only)
    ChallengeStartLaunchCountdown       = 3012    (protobuf only)
    ChallengeSetPlayerSettings          = 3013    (protobuf only)

    --- External Chat ---
    ExternalChatLogin                   = 4000    (protobuf only)
    ExternalChatAuthGetToken            = 4001    (protobuf only)
    ExternalChatMerge                   = 4002    (protobuf only)
    ExternalChatUnmerge                 = 4003    (protobuf only)

    ProtobufTest                        = 9991


================================================================================
SECTION 2: LOGGABLE MESSAGES (Player.log whitelist)
================================================================================

Defined in FrontDoorConnectionAWS.LoggableMessages (28 commands).
Only these CmdTypes produce ==> / <== entries in Player.log.

    DeckUpsertDeckV3            (412)
    DeckGetDeck                 (400)
    DeckGetDeckSummariesV3      (411)
    DeckGetPreconDeckV2         (409)
    DeckGetAllPreconDecksV3     (410)
    DeckDeleteDeck              (403)
    DcIssueChallenge            (500)
    BotDraftDraftStatus         (1802)
    BotDraftDraftPick           (1801)
    EventPlayerDraftMakePick    (620)
    EventAiBotMatch             (612)
    EventClaimPrize             (607)
    DraftCompleteDraft          (1908)
    EventSetDeckV3              (627)
    EventGetActiveEvents        (604)
    EventGetActiveMatches       (613)
    RankGetCombinedRankInfo     (1100)
    EventGetCoursesV2           (623)
    RankGetSeasonAndRankDetails (1102)
    EventJoin                   (600)
    EventEnterPairing           (603)
    LogBusinessEvents           (1913)
    GetFormats                  (6)
    StartHook                   (1)
    PeriodicRewardsGetStatus    (1200)
    QuestGetQuests              (1000)
    GraphProcess                (1702)
    GraphGetGraphState          (1701)

NOT logged (relevant to collection tracking):
    Card_GetCardSet             (550)
    Card_GetAllCards            (551)
    Card_RedeemWildCards        (552)
    Card_CompleteVault          (553)
    Currency_GetCurrencies      (800)
    Booster_OpenBoosters        (900)
    Booster_GetOwnedBoosters    (901)
    Cosmetics_*                 (1900-1909)
    Merc_*                      (705-715)
    Store_*                     (703, 712)

Debounce commands (rate-limited, only one in-flight at a time):
    EventJoin, EventJoinDraftQueue, EventJoinDraft, EventDrop,
    MarkLetterRead, ClaimAttachment

Message priority system (FrontDoorConnectionAWS):
    - Debounce: above commands, only one in-flight per CmdType
    - BypassQueue: LogBusinessEvents, LogBusinessEventsV2 (skip the queue)
    - Standard: everything else, queued FIFO

SendMessage signatures:
    Promise<TResponse> SendMessage<TResponse>(CmdType, object request = null)
    Promise<TResponse> SendMessage<TResponse>(CmdType, IMessage request = null)  // protobuf
    Promise<TResponse> SendMessage<TResponse>(Func<byte[],TResponse>, CmdType, object request)

Logging behavior:
    - Non-debug accounts: only LoggableMessages are written to Player.log
    - Debug accounts: ALL messages are logged (isDebugAccount flag)
    - Push notifications: only logged for debug accounts (LogPushNotification)
    - InventoryChanged push: visible indirectly via OnMsg_InventoryInfoUpdated_AWS handler


================================================================================
SECTION 3: PUSH NOTIFICATION TYPES
================================================================================

ENotificationType enum (Wizards.Arena.Enums.Player):
    Invalid                         = 0
    ConfigMismatch                  = 1       data: DirectChallengeMismatchReasons (EMismatchReason[])
    ForceDropped                    = 2       data: (none, silently dropped)
    InventoryChanged                = 3       data: InventoryInfo (full inventory snapshot + Changes[])
    SystemMessage                   = 4       data: SystemMessage[]
    FriendRequestNotification       = 100     data: (none, just a signal)
    MatchCreated                    = 600     data: ClientMatchInfoV2 (match endpoint, players, seat)
    ErrMatchCreate                  = 9000    data: (error variant)
    TencentSecurity                 = 10000   data: TencentSecurityBlob (byte[])
    DraftNotification               = 11000   data: DraftNotification
    PodNotification                 = 12000   data: PodQueueNotification
    KillSwitchNotification          = 13000   data: DTO_KillSwitchNotification (feature flags)
    TournamentState                 = 13001   data: TournamentState
    StaticContentNotification       = 14000   data: StaticContentResponse
    CampaignGraphDeltaNotification  = 15000   data: CampaignGraphDeltas (mastery/battle pass progress)
    LobbyStatus                     = 16000   (obsolete, use LobbyNotification)
    LobbyMessage                    = 16001   (obsolete, use LobbyNotification)
    LobbyNotification               = 16002   data: LobbyNotification (sub-typed by ELobbyNotificationType)
    TournamentNotification          = 17000   data: TournamentNotification
    GatheringNotification           = 18000   data: GatheringNotification
    ChallengeNotification           = 19000   data: ChallengeNotification

PushNotification class fields (Wizards.Unification.Models.Player):
    Type:                           ENotificationType
    MatchInfoV3:                    ClientMatchInfoV2
    InventoryInfo:                  InventoryInfo
    TencentSecurityBlob:            byte[]
    systemMessage:                  SystemMessage          (deprecated)
    SystemMessages:                 SystemMessage[]
    DirectChallengeMismatchReasons: EMismatchReason[]
    DraftNotification:              DraftNotification
    PodQueueNotification:           PodQueueNotification
    KillSwitchNotification:         DTO_KillSwitchNotification
    StaticContentNotification:      StaticContentResponse
    CampaignGraphDeltas:            CampaignGraphDeltas
    LobbyNotification:              LobbyNotification
    GatheringNotification:          GatheringNotification
    ChallengeNotification:          ChallengeNotification
    TournamentNotification:         TournamentNotification
    TournamentStateNotification:    TournamentState

ProcessIncomingMessage dispatch (FrontDoorConnectionAWS):
    MatchCreated        -> OnMsg_Event_MatchCreated (builds NewMatchCreatedConfig)
    InventoryChanged    -> OnMsg_InventoryInfoUpdated_AWS (converts via AWSInventoryConversions)
    SystemMessage       -> OnMsg_System_Message
    ConfigMismatch      -> OnMsg_Challenge_ForcedDrop
    DraftNotification   -> OnMsg_DraftNotification
    PodNotification     -> OnMsg_PodQueueNotification
    KillSwitchNotification -> OnMsg_KillSwitchNotification (updates local Killswitch)
    StaticContentNotification -> OnMsg_StaticContentNotification
    FriendRequestNotification -> OnMsg_FriendRequestNotification
    CampaignGraphDeltaNotification -> OnMsg_CampaignGraphDeltas
    LobbyNotification   -> InvokeLobbyNotification (sub-dispatches by ELobbyNotificationType)
    TournamentNotification -> InvokeTournamentNotification
    ChallengeNotification -> OnMsg_ChallengeNotification
    ForceDropped        -> (silently ignored)
    GatheringNotification -> NOT HANDLED (no case in switch)
    TournamentState     -> NOT HANDLED (no case in switch)
    ErrMatchCreate      -> NOT HANDLED
    TencentSecurity     -> NOT HANDLED
    default             -> warning logged

Sub-notification type enums:

    ELobbyNotificationType (in LobbyNotification):
        Invalid, LobbyStatus, LobbyChatMessage, LobbyInvite, LobbyClose, LobbyKick

    ETournamentNotificationType (in TournamentNotification):
        Invalid, TournamentIsReady, RoundStarted, RoundReadyUpdate, OtherMatchCompletedUpdate

    EGatheringNotificationType (in GatheringNotification):
        Invalid, GatheringStatus, GatheringChatMessage, GatheringInvite, GatheringClose, GatheringKick

    EChallengeNotificationType (in ChallengeNotification):
        Invalid, ChallengeStatus, ChallengeChatMessage, ChallengeInvite,
        ChallengeClose, ChallengeKick, ChallengeStartLaunchCountdown

    EDraftNotificationType (in DraftNotification):
        ReadyReq, ReadyUpdate, TableInfo, PickReq, Close, PackInfo


================================================================================
SECTION 4: INVENTORY DATA PIPELINE
================================================================================

--- 4a. InventoryInfo (server -> client, JSON inside protobuf) ---
Class: Wizards.Unification.Models.Player.InventoryInfo
    SeqId:              int                     sequence number for ordering
    Changes:            List<InventoryChange>   what changed and why
    Gems:               int                     total gems balance
    Gold:               int                     total gold balance
    TotalVaultProgress: int                     raw vault progress (divide by 10 for %)
    wcTrackPosition:    int                     position on wildcard wheel
    WildCardCommons:    int                     total common wildcards
    WildCardUnCommons:  int                     total uncommon wildcards
    WildCardRares:      int                     (always 0 in current data — see note)
    WildCardMythics:    int                     total mythic wildcards
    CustomTokens:       Dict<string,int>        e.g. {"DraftToken":2, "SealedToken":0, "Token_JumpIn":5, "BattlePass_FIN":1}
    Boosters:           List<BoosterStack>      [{CollationId, SetCode, Count}, ...]
    Vouchers:           Dict<Guid,int>          voucher stacks
    PrizeWallsUnlocked: HashSet<string>         unlocked prize walls
    Cosmetics:          CosmeticsClient         owned cosmetics (art styles, avatars, pets, sleeves, emotes, titles)

NOTE: WildCardRares is present in the class but the JSON field in Player.log
uses "WildCardRares" — check actual log output to confirm the JSON key name
matches. The StartHook response uses these same fields.

--- 4b. InventoryChange (individual change within InventoryInfo) ---
Class: Wizards.Unification.Models.Player.InventoryChange
    Source:             InventoryChangeSource   why the change happened
    SourceId:           string                  event/quest/graph ID
    InventoryGold:      int                     gold delta
    InventoryGems:      int                     gems delta
    InventoryCustomTokens: Dict<string,int>     token deltas
    ArtStyles:          List<DTO_CosmeticArtStyleEntry>
    Avatars:            List<string>
    Sleeves:            List<string>
    Pets:               List<string>
    Emotes:             List<string>
    Titles:             List<string>
    WildCardTrackUnCommons: int                 wc track progress (uncommon)
    WildCardTrackRares:     int                 wc track progress (rare)
    WildCardTrackMythics:   int                 wc track progress (mythic)
    SetMasteryProgress:     int                 XP gained
    WildCardCommons:    int                     wildcards granted
    WildCardUnCommons:  int
    WildCardRares:      int
    WildCardMythics:    int
    Decks:              List<DTO_DeckSummary>    (obsolete)
    DecksV2:            List<DTO_DeckSummaryV2>
    DecksV3:            List<DTO_DeckSummaryV3>
    DeckCards:          Dict<Guid,List<DeckCard>>
    Boosters:           List<BoosterStack>
    GrantedCards:        List<GrantedCard>        cards granted (with aetherization info)
    Vouchers:           Dict<Guid,int>
    NewLetters:         List<Letter>
    PrizeWallsUnlocked: HashSet<string>

--- 4c. InventoryChangeSource enum (server-side, in Changes[].Source) ---
Enum: Wizards.Arena.Enums.Player.InventoryChangeSource
    Unknown
    QuestReward
    DailyWins
    WeeklyWins
    LoginGrant
    IdEmpotentLoginGrant
    BattlePassLevelUp
    BattlePassLevelMasteryTree
    EarlyPlayerProgressionLevelUp
    EarlyPlayerProgressionMasteryTree
    ProgressionRewardTierAdd
    RenewalReward
    MercantilePurchase
    MercantileChestPurchase
    MercantileBoosterPurchase
    EventReward
    RedeemVoucher
    ModifyPlayerInventory
    OpenChest
    MassOpenChest
    BasicLandSetUpdate
    CompleteVault
    CosmeticPurchase
    WildCardRedemption
    BoosterOpen
    StarterDeckUpgrade
    RankedSeasonReward
    EventPayEntry
    BannedCardGrant
    RestrictedCardGrant
    CustomerSupportGrant
    EntryReward
    CampaignGraphPayoutNode
    EventGrantCardPool
    CampaignGraphAutomaticPayoutNode
    CampaignGraphPurchaseNode
    CampaignGraphTieredRewardNode
    AccumulativePayoutNode
    EventRefundEntry
    Cleanup
    Letter
    PrizeWallPurchase
    CrossPlatformReward

--- 4d. InventoryUpdateSource enum (client-side, after conversion) ---
Enum: Wizards.Models.InventoryUpdateSource (54 values)
    Differences from InventoryChangeSource (52 values):
      Renamed:
        EntryReward                   -> EventEntryReward
        CampaignGraphPurchaseNode     -> CatalogPurchase
        CampaignGraphPayoutNode       -> CampaignGraphReward
      Only in AWS (InventoryChangeSource):
        PrizeWallPurchase             (value 36, no Azure equivalent)
      Only in Azure (InventoryUpdateSource):
        CatalogPurchase               (distinct from the rename above — separate entry)
        CampaignGraphReward           (distinct from the rename above — separate entry)
    (see AWSInventoryConversions.ConvertToAzureSource() for the full mapping)

--- 4e. InventoryDelta (client-side processed delta) ---
Class: Wizards.Mtga.FrontDoorModels.InventoryDelta
    gemsDelta:          int
    goldDelta:          int
    boosterDelta:       BoosterStack[]
    cardsAdded:         int[]              (GrpId array)
    decksAdded:         Guid[]
    vanityItemsAdded:   string[]
    vanityItemsRemoved: string[]
    vaultProgressDelta: decimal
    wcTrackPosition:    int
    wcCommonDelta:      int
    wcUncommonDelta:    int
    wcRareDelta:        int
    wcMythicDelta:      int
    artSkinsAdded:      ArtSkin[]
    artSkinsRemoved:    ArtSkin[]
    voucherItemsDelta:  VoucherStack[]
    tickets:            TicketStack[]
    customTokenDelta:   CustomTokenDeltaInfo[]
    newLetters:         List<Client_Letter>

--- 4f. InventoryChangeShared (intermediate conversion) ---
Class: Wizards.Mtga.FrontDoorModels.InventoryChangeShared
    Source:                 InventoryChangeSource
    InventoryGold:          int
    InventoryGems:          int
    InventoryDraftTokens:   int
    InventorySealedTokens:  int
    InventoryCustomTokens:  Dict<string,CustomTokenDeltaInfo>
    ArtStyles:              List<DTO_CosmeticArtStyleEntry>
    Avatars:                List<string>
    Sleeves:                List<string>
    Pets:                   List<string>
    Emotes:                 List<string>
    Titles:                 List<string>
    WildCardTrackUnCommons: int
    WildCardTrackRares:     int
    WildCardTrackMythics:   int
    WildCardCommons:        int
    WildCardUnCommons:      int
    WildCardRares:          int
    WildCardMythics:        int
    SetMasteryProgress:     int
    Decks:                  List<DTO_DeckSummaryV3>
    DeckCards:              Dict<Guid,List<DeckCard>>
    Boosters:               List<BoosterStackShared>
    Vouchers:               List<VoucherStackShared>
    GrantedCards:            List<GrantedCardShared>
    NewLetters:             List<Letter>

--- 4g. GrantedCardShared (card grant with aetherization info) ---
Class: Wizards.Mtga.FrontDoorModels.GrantedCardShared
    GrpId:              int         card GRP ID
    CardAdded:          bool        true if actually added to collection (false = 5th+ copy)
    SetCode:            string      expansion code
    IsGrantedFromDeck:  bool        true if granted via starter deck
    Gems:               int         gems awarded instead (5th copy protection)
    VaultProgress:      int         vault progress awarded instead

NOTE: The server-side GrantedCard (Wizards.Unification.Models.Player.GrantedCard)
uses the field name "IsGrantedWithDeck" for the same concept. The conversion in
AWSInventoryConversions maps IsGrantedWithDeck -> IsGrantedFromDeck.

--- 4h. AetherizedCardInformation (UI-facing card grant info) ---
Class: Wizards.Mtga.FrontDoorModels.AetherizedCardInformation
    grpId:              int
    addedToInventory:   bool
    isGrantedFromDeck:  bool
    vaultProgress:      float
    goldAwarded:        int
    gemsAwarded:        int
    set:                string

--- 4i. ClientInventoryUpdateReportItem (final UI report) ---
Class: Wizards.Mtga.FrontDoorModels.ClientInventoryUpdateReportItem
    delta:              InventoryDelta
    aetherizedCards:    List<AetherizedCardInformation>
    xpGained:           int
    context:            InventoryUpdateContext
    parentcontext:      string

--- 4j. Conversion pipeline ---
    Server sends InventoryInfo (inside PushNotification or API response)
        -> AWSInventoryConversions.ConvertInventoryInfo() converts to InventoryInfoShared
        -> InventoryInfoShared.ToUpdateReportItem() converts each Change to
           ClientInventoryUpdateReportItem (with InventoryDelta + AetherizedCardInformation)
        -> UI consumes ClientInventoryUpdateReportItem

--- 4k. ProcessDelta (card grant processing) ---
Method: AwsInventoryServiceWrapper.ProcessDelta()
    Location: SharedClientCore.dll, lines ~191-226
    Called after inventory update to apply card grants to local state.
    For each GrantedCard in the change:
        1. Skips if GrpId matches any wildcard ID (common/uncommon/rare/mythic)
        2. If CardAdded == true:
            - Increments Cards[GrpId] in ClientPlayerInventory
            - Adds GrpId to InventoryDelta.cardsAdded
        3. Builds AetherizedCardInformation from the GrantedCard fields
           (addedToInventory, gemsAwarded, vaultProgress, set, isGrantedFromDeck)
    Returns: List<AetherizedCardInformation> for UI display

--- 4l. ProcessInventoryUpdate (main inventory update handler) ---
Method: AwsInventoryServiceWrapper.ProcessInventoryUpdate()
    Location: SharedClientCore.dll, lines ~233-262
    Called when server pushes InventoryInfo (via notification or API response).
    Steps:
        1. Updates ClientPlayerInventory scalar fields:
           gold, gems, wcCommon, wcUncommon, wcRare, wcMythic,
           vaultProgress, wcTrackPosition, boosters, vouchers,
           CustomTokens, prizeWallsUnlocked
        2. For each Change in InventoryInfo.Changes:
            - Converts to InventoryChangeShared via AWSInventoryConversions
            - Calls ProcessDelta() for card grants
            - Builds ClientInventoryUpdateReportItem
        3. Fires inventory-changed callbacks (UI observers)
    This is the single choke-point where ALL inventory mutations flow through.

--- 4m. ClientPlayerInventory (local inventory state) ---
Class: Wizards.Mtga.Inventory.ClientPlayerInventory
    wcCommon:           int
    wcUncommon:         int
    wcRare:             int
    wcMythic:           int
    gold:               int
    gems:               int
    wcTrackPosition:    int
    vaultProgress:      double
    boosters:           List<ClientBoosterInfo>
    vouchers:           List<ClientVoucherDescription>
    prizeWallsUnlocked: HashSet<string>
    basicLandSet:       string
    latestBasicLandSet: string
    starterDecks:       Guid[]
    tickets:            TicketStack[]
    CustomTokens:       Dict<string,int>
    draftTokens:        int (computed from CustomTokens["DraftToken"])
    sealedTokens:       int (computed from CustomTokens["SealedToken"])

--- 4n. BoosterStack ---
Class: Wizards.Unification.Models.Booster.BoosterStack
Class: Wizards.Mtga.FrontDoorModels.BoosterStackShared
    CollationId:        int         numeric ID for the set/booster type
    SetCode:            string      e.g. "FDN", "MH3"
    Count:              int

--- 4o. CosmeticsClient ---
Class: Wizards.Models.CosmeticsClient
    ArtStyles:          List<DTO_CosmeticArtStyleEntry>
    Avatars:            List<CosmeticsAvatarEntry>
    Pets:               List<CosmeticPetEntry>
    Sleeves:            List<CosmeticsSleeveEntry>
    Emotes:             List<CosmeticEmoteEntry>
    Titles:             List<CosmeticTitleEntry>


================================================================================
SECTION 5: PLAYER.LOG EVENT CATALOG
================================================================================

Parsed from Player.log: 94,213 lines, 3,421 JSON structures, 263 API calls.

--- 5a. API calls (==> requests) — 16 distinct call names ---
    EventGetCoursesV2:           51x
    GraphGetGraphState:          43x
    PeriodicRewardsGetStatus:    27x
    QuestGetQuests:              26x
    RankGetCombinedRankInfo:     17x
    StartHook:                   16x    (session init, contains full inventory)
    RankGetSeasonAndRankDetails: 16x
    DeckUpsertDeckV3:            15x
    EventSetDeckV3:              13x
    EventGetActiveMatches:       11x
    EventEnterPairing:           10x
    EventJoin:                   9x
    DeckGetDeckSummariesV3:      5x
    EventClaimPrize:             2x
    GetFormats:                  1x
    DeckGetAllPreconDecksV3:     1x

NOTE: Response lines use format "<== ApiName(guid)" where guid is the transaction
ID. All 16 request types above have matching responses in the log.

--- 5b. JSON structures by key signature (22 distinct, by frequency) ---

    1. greToClientEvent (match gameplay messages)
       Keys: transactionId, requestId, timestamp, greToClientEvent
       Count: 2771, Size: 271-105,275 bytes
       These are GRE (Game Rules Engine) messages — game state, actions, etc.

    2. greToClientEvent (no transaction)
       Keys: timestamp, greToClientEvent
       Count: 350, Size: 1,072-19,454 bytes
       Gameplay state updates without request context.

    3. Courses (event listings)
       Keys: Courses
       Count: 51, Size: 6,338-12,777 bytes
       Response to EventGetCoursesV2. Lists all active events with deck info.

    4. GraphState (campaign/mastery state)
       Keys: NodeStates, MilestoneStates
       Count: 43, Size: 241-6,362 bytes
       Response to GraphGetGraphState. Mastery pass / battle pass progress.

    5. PeriodicRewards
       Keys: _dailyRewardSequenceId, _dailyRewardResetTimestamp,
             _weeklyRewardSequenceId, _weeklyRewardResetTimestamp,
             _dailyRewardChestDescriptions, _weeklyRewardChestDescriptions
       Count: 27, Size: 5,811 bytes (fixed)
       Response to PeriodicRewardsGetStatus.

    6. Quests
       Keys: canSwap, quests
       Count: 26, Size: 28 bytes
       Response to QuestGetQuests. In this log, quests array is always empty
       (quests completed).

    7. MatchGameRoomStateChanged
       Keys: transactionId, requestId, timestamp, matchGameRoomStateChangedEvent
       Count: 24, Size: 1,024-1,266 bytes
       Match room state changes (player joining, match config).

    8. StartHook response (BIGGEST — full inventory snapshot)
       Keys: InventoryInfo, DeckSummaries, Decks, SystemMessages,
             PreferredCosmetics, HomePageAchievements, DeckLimit,
             TimewalkInfo, TokenDefinitions, KillSwitchNotification,
             CardMetadataInfo, ClientPeriodicRewards, UpdatedGraphs, ServerTime
       Count: 16, Size: 227,892-229,869 bytes
       THIS IS THE MOTHER LODE. Contains complete inventory state on every
       session start. ~228 KB per occurrence.

    9. RankInfo
       Keys: currentSeason, limitedRankInfo, constructedRankInfo
       Count: 16, Size: 6,739 bytes
       Response to RankGetCombinedRankInfo.

    10. DeckUpsert response
        Keys: DeckId, Name, Attributes, DeckTileId, DeckArtId, PreferredCosmetics
        Count: 14, Size: 488-521 bytes

    11. AuthenticateResponse
        Keys: transactionId, requestId, timestamp, authenticateResponse
        Count: 13, Size: 262 bytes
        Contains clientId and sessionId.

    12. ActiveMatches
        Keys: MatchesV3
        Count: 11, Size: 16 bytes (usually empty)

    13. EventJoin response (Course + InventoryInfo)
        Keys: Course, InventoryInfo
        Count: 11, Size: 4,280-7,384 bytes
        Returned when joining an event. Contains entry fee deduction.

    14. RankProgress (compact)
        Keys: constructedSeasonOrdinal, constructedLevel, constructedStep,
              constructedMatchesWon, limitedSeasonOrdinal, limitedLevel
        Count: 11, Size: 141 bytes

    15. Course detail
        Keys: CourseId, InternalEventName, CurrentModule, ModulePayload,
              CourseDeckSummary, CourseDeck, CardPool, CardStyles
        Count: 10

    16. Module change
        Keys: CurrentModule, Payload
        Count: 10, Size: 51 bytes
        Module transitions within an event ("CreateMatch" -> "Success").

    17. Formats
        Keys: Formats, FormatGroups
        Count: 1, Size: 470,140 bytes
        Response to GetFormats. Complete legal set/format definitions.

    18. PreconDecks
        Keys: CacheVersion, PreconDecks
        Count: 1, Size: 1,014,637 bytes (~1 MB!)
        Response to DeckGetAllPreconDecksV3. All starter/precon decks.

--- 5c. Log line prefixes (non-JSON, top patterns) ---
    Can:            570x   (Unity rendering/capability messages)
    Unloading:      109x   (scene unloads)
    Parent:         107x   (Unity hierarchy)
    OnSceneLoaded:  96x
    OnSceneUnloaded: 81x
    Peak:           81x    (memory stats)
    cannot:         60x
    Overflow:       48x    (buffer overflow warnings)
    Initial:        44x    (initialization logs)
    Current:        40x    (current state logs)
    BEGIN/END:      26x each (operation markers)


================================================================================
SECTION 6: CARD DATABASE SCHEMA (SQLite)
================================================================================

File: Raw_CardDatabase_*.mtga (226.89 MB)
Version: Data 2026.59.0.124, GRP 124.1273149

--- 6a. Tables (17 total) ---

    Cards (25,017 rows) — MAIN TABLE
        GrpId:              INT [PK]    THE card ID used everywhere
        ArtId:              INT         art asset reference
        TitleId:            INT         -> Localizations (card name)
        AltTitleId:         INT         alternate title (0 if none)
        InterchangeableTitleId: INT     for functional reprints
        FlavorTextId:       INT         -> Localizations
        ReminderTextId:     INT         -> Localizations
        TypeTextId:         INT         -> Localizations (type line)
        SubtypeTextId:      INT         -> Localizations
        ArtistCredit:       TEXT
        ArtSize:            INT
        Rarity:             INT         1=token? 2=common, 3=uncommon, 4=rare, 5=mythic
        ExpansionCode:      TEXT        e.g. "FDN", "MH3", "10E"
        DigitalReleaseSet:  TEXT        actual Arena release set (e.g. "AA4", "LIST-MKM", "Y22-MID")
        IsToken:            BOOLEAN
        IsPrimaryCard:      BOOLEAN     true for main printing
        IsDigitalOnly:      BOOLEAN
        IsRebalanced:       BOOLEAN
        RebalancedCardGrpId: INT        GrpId of rebalanced version (0 if none)
        DefunctRebalancedCardGrpId: INT old rebalanced version
        AlternateDeckLimit: INT         0 = normal (4), nonzero = special limit
        CollectorNumber:    TEXT
        CollectorMax:       TEXT
        CollectorSuffix:    TEXT
        DraftContent:       BOOLEAN     whether card appears in draft boosters
        UsesSideboard:      BOOLEAN
        OldSchoolManaText:  TEXT        e.g. "o2oW" = {2}{W}
        LinkedFaceType:     INT         0=none, nonzero=DFC/adventure/etc
        RawFrameDetail:     TEXT        frame color codes ("W","B","M","A","L")
        Power:              TEXT
        Toughness:          TEXT
        Colors:             TEXT        comma-separated color IDs ("1" = white, etc)
        ColorIdentity:      TEXT
        FrameColors:        TEXT
        Types:              TEXT        comma-separated type IDs (see Enums.CardType)
        Subtypes:           TEXT        comma-separated subtype IDs (see Enums.SubType)
        Supertypes:         TEXT        comma-separated supertype IDs
        AbilityIds:         TEXT        format: "abilityId:abilityNumericId,..."
        LinkedFaceGrpIds:   TEXT        GrpIds of other faces
        KnownSupportedStyles: TEXT      e.g. "DA" (default art), can include others
        Tags:               TEXT        comma-separated tag hashes
        PromoLabel:         TEXT
        Order_* columns:    INT/TEXT    sorting helpers (LandLast, ColorOrder, etc.)

    Abilities (20,117 rows)
        Id:                 INT [PK]
        BaseId:             INT
        TextId:             INT         -> Localizations (ability text)
        Category:           INT         ability category
        SubCategory:        INT
        PaymentType:        INT
        LoyaltyCost:        TEXT
        Power/Toughness:    TEXT        (for P/T-setting abilities)
        FullyParsed:        BOOLEAN
        IsIntrinsicAbility: BOOLEAN
        Tags, CostTypes, RelevantZones, etc.

    AltPrintings (3,627 rows)
        AltGrpId:           INT [PK]    alternate art GrpId
        IsAltArt:           BOOLEAN     true = alt art, false = alt frame
        SkinCode:           TEXT        e.g. "FF", "FF_AA1", "FF_CUTE"

    AltToBasePrintings (4,795 rows)
        BaseGrpId:          INT [PK]    base printing GrpId
        AltGrpId:           INT [PK]    alt printing GrpId
        Maps alternate printings back to their base card.

    Enums (834 rows) — GAME ENUM DEFINITIONS
        Type:               TEXT [PK]   enum name
        Value:              INT [PK]    numeric value
        LocId:              INT         -> Localizations (display name)
        Enum types: CardColor(8), CardType(14), Color(5), CounterType(214),
                    FailureReason(28), MatchState(5), Phase(6), ResultCode(14),
                    Step(12), SubType(523), SuperType(5)
        NOTE: NO collation-to-set mapping exists in the Enums table.

    Localizations (62,519 rows) — master LocId table
        LocId:              INT [PK]

    Localizations_enUS (101,601 rows) — English text
    Localizations_deDE (121,954 rows)
    Localizations_esES (108,725 rows)
    Localizations_frFR (124,853 rows)
    Localizations_itIT (99,509 rows)
    Localizations_jaJP (160,729 rows)
    Localizations_koKR (131,793 rows)
    Localizations_phyrexian (62,581 rows) — yes, Phyrexian is a supported language
    Localizations_ptBR (119,380 rows)
        Each has: LocId, Formatted (0=raw, 1=formatted, 2=accent-stripped), Loc (text)

    Prompts (3,323 rows) — UI prompt definitions
        Id:                 INT [PK]
        LocId:              INT

    Versions (2 rows)
        Type:               TEXT [PK]    "Data" or "GRP"
        Version:            TEXT         e.g. "2026.59.0.124"

--- 6b. Indexes ---
    Cards: idx_card_title, idx_card_interchangeableTitle, idx_card_artId,
           idx_card_expansionCode, idx_card_isPrimaryCard
    Enums: idx_enum (Type, Value)
    Localizations_*: idx_loc_XX on Loc column

--- 6c. Notable data we may not be extracting ---
    - AltPrintings + AltToBasePrintings: Maps alt art/frame variants to base cards.
      Useful for collection completion tracking across printings.
    - Abilities table: Full ability text and categorization. Could be used for
      card search/filtering.
    - Cards.DigitalReleaseSet: Tells you which Arena set a card actually belongs
      to (may differ from ExpansionCode for anthologies, lists, etc).
    - Cards.IsRebalanced / RebalancedCardGrpId: Maps original <-> rebalanced cards.
    - Cards.KnownSupportedStyles: Card styles available (parallax, etc).
    - Cards.Tags: Hashed tag IDs — meaning unknown without the tag definition table.

--- 6d. Collation ID mapping ---
    The card database does NOT contain a collation-to-set mapping.
    Collation IDs appear in BoosterStack.CollationId and are numeric IDs that
    map to sets (e.g., collation ID 100037 = "FDN"). The mapping is provided by:
        1. GetSets command (CmdType 1521) — not logged
        2. VoucherDefinitions (CmdType 1520) — has CollationId field
        3. The BoosterStackShared class includes BOTH CollationId AND SetCode,
           so the mapping can be inferred from inventory data.

--- 6e. Top expansion codes by card count ---
    J25: 841    FDN: 646    MH3: 538    HBG: 525    JMP: 524
    FIN: 517    NEO: 501    VOW: 454    TDM: 452    DSK: 448
    KHM: 444    MOM: 435    MID: 432    SNC: 428    AKR: 415
    WOE: 415    MKM: 413    AFR: 412    ELD: 412    BLB: 407
    (50 total distinct expansion codes in database)


================================================================================
SECTION 7: MTGA FILE SYSTEM
================================================================================

--- 7a. Install directory structure ---
    MTGA_Data/
        Downloads/          26,023 files, ~13 GB total
            ALT/            48 files, 22.9 MB      (alternate asset lookup tables)
            AssetBundle/    25,798 files, 11,140 MB (card art, UI assets, etc)
            Audio/          168 files, 1,434 MB     (.pck audio banks)
            Raw/            6 files, 255 MB         (card DB, localizations, credits)
        Logs/               27 files
            DownloadLogs/   download timing + asset lists
            Logs/           UTC_Log files (detailed game logs, rotated per session)
        Managed/            228 files (.NET assemblies)
        Plugins/            133 files (native DLLs)
        Resources/          3 files
        StreamingAssets/     11 files (EOS config, Unity services config)

--- 7b. Downloads/Raw files ---
    Raw_CardDatabase_*.mtga             226.89 MB   SQLite card database
    Raw_ClientLocalization_*.mtga       22.44 MB    client UI translations
    Raw_ArtCropDatabase_*.mtga          5.75 MB     art crop coordinates
    Raw_altArtCredits_*.mtga            ~0 MB       alt art credit overrides
    Raw_altFlavorTexts_*.mtga           ~0 MB       alt flavor text overrides
    Raw_credits_*.mtga                  0.02 MB     game credits

--- 7c. Downloads/ALT files (notable) ---
    ALT_AssetLookupTree_*.mtga          asset lookup index
    ALT_Booster_*.mtga                  booster pack assets
    ALT_Avatar_*.mtga                   avatar assets
    ALT_Battlefield_*.mtga              battlefield assets
    ALT_CardHolder_*.mtga               card sleeve assets
    (48 files total, mostly UI/visual asset indexes)

--- 7d. Manifest files ---
    Manifest_*.mtga             12.87 MB    JSON asset manifest (25,798+ entries)
        Format: {"Assets": [{"Name": "...", "Length": N, "sha256": "...", ...}, ...]}
        Each entry: AssetBundle name, size, hash, priority
    Manifest_Audio_*.mtga       0.04 MB     audio manifest
    Manifest_Localization_*.mtga 0.00 MB    localization manifest

--- 7e. Logs directory ---
    MTGA_Data/Logs/DownloadLogs/
        downloads_YYYYMMDD_HHMMSS.log   ~1.6 MB each, 12+ files
            Asset download tracking, one per session
        Boot_timings_*.log              startup timing
        General_timings_*.log           general performance timing

    MTGA_Data/Logs/Logs/
        UTC_Log - MM-DD-YYYY HH.MM.SS.log    3-43 MB each
            Detailed game engine logs. These are NOT the same as Player.log.
            They contain Unity engine output, rendering info, matchmaking details,
            and GRE (Game Rules Engine) protocol messages.
            Rotated per session, ~12 files retained.

--- 7f. AppData directory ---
    %LOCALAPPDATA%/../LocalLow/Wizards Of The Coast/MTGA/
        Player.log          ~28 MB      THE log file we parse
        Player-prev.log     ~43 MB      previous session's log
        Unity/              analytics data (config, values, archived events)

    NO local collection/inventory cache files were found.
    The game does not persist collection data to disk — it fetches everything
    fresh via StartHook on each session start.

--- 7g. Config files in MTGA_Data ---
    boot.config                                 Unity boot configuration
    RuntimeInitializeOnLoads.json               Unity runtime init
    ScriptingAssemblies.json                    Assembly load list
    StreamingAssets/EpicOnlineServicesConfig.json    EOS config
    StreamingAssets/UnityServicesProjectConfiguration.json   Unity services

    None of these contain player data.

--- 7h. Raw_ArtCropDatabase (potentially useful) ---
    5.75 MB SQLite file with art crop coordinate data for card rendering.
    Could be useful for building a card image display system but not needed
    for collection tracking.


================================================================================
SECTION 8: STARTHOOK RESPONSE STRUCTURE
================================================================================

The StartHook response is the single most valuable data source. It contains
a complete inventory snapshot sent on every session start.

Observed keys (from Player.log analysis):
    InventoryInfo           Full inventory (see Section 4a)
    DeckSummaries           All player decks (summary format)
    Decks                   (possibly detailed deck data)
    SystemMessages          Active system messages
    PreferredCosmetics      Player's cosmetic preferences
    HomePageAchievements    Achievement progress for home page
    DeckLimit               Maximum number of decks
    TimewalkInfo            Timewalk/historic brawl info
    TokenDefinitions        Custom token definitions (DraftToken, SealedToken, etc)
    KillSwitchNotification  Feature flag states
    CardMetadataInfo        Card metadata (rebalanced card mappings?)
    ClientPeriodicRewards   Daily/weekly reward status
    UpdatedGraphs           Campaign graph state updates
    ServerTime              Current server timestamp

Size: ~228 KB per StartHook response.
Frequency: Once per session start (16x in the analyzed log).


================================================================================
SECTION 9: KEY OBSERVATIONS FOR TRACKER DEVELOPMENT
================================================================================

1. COLLECTION DATA IS NOT LOGGED
   Card_GetAllCards (551) and Card_GetCardSet (550) are NOT in LoggableMessages.
   The only way to get collection data from Player.log is via:
   - StartHook response -> InventoryInfo (inventory totals only, not card-by-card)
   - InventoryChanged push notifications -> Changes[].GrantedCards (new cards)
   You CANNOT get the full collection from Player.log alone.

2. BOOSTER OPENS ARE PARTIALLY VISIBLE
   Booster_OpenBoosters (900) is NOT logged, but the resulting InventoryChanged
   push notification IS visible. The GrantedCards array in the push notification
   shows exactly which cards were opened and whether they were added to inventory
   or converted to gems/vault progress (5th copy protection).

3. WILDCARD CRAFTING IS PARTIALLY VISIBLE
   Card_RedeemWildCards (552) is NOT logged, but the resulting InventoryChanged
   push with Source=WildCardRedemption IS visible.

4. THE STARTHOOK IS YOUR BEST FRIEND
   StartHook runs on every session start and returns ~228 KB of data including
   full inventory, deck lists, quests, rank, mastery progress, etc.

5. COLLATION IDs COME WITH SET CODES
   BoosterStack/BoosterStackShared include BOTH CollationId and SetCode,
   so you can build the mapping from observed inventory data without needing
   the GetSets API call.

6. NO LOCAL CACHE
   MTGA does not cache collection/inventory data locally. Every session starts
   fresh from the server. There's no SQLite or JSON file on disk with your
   collection.

7. UTC_LOG FILES ARE DIFFERENT FROM PLAYER.LOG
   The Logs/Logs/ directory contains detailed Unity/GRE logs that are separate
   from Player.log. These contain match gameplay details but in a different
   format. Player.log is the standard source for API message logging.

8. CARD DATABASE IS COMPREHENSIVE BUT READ-ONLY
   The 227 MB SQLite database contains all card definitions, abilities,
   localizations in 9 languages (including Phyrexian), and alternate printings.
   It does NOT contain player-specific data (collection, inventory).

9. ALTERNATE PRINTINGS ARE MAPPED
   AltPrintings + AltToBasePrintings tables map alternate arts/frames to their
   base card, with skin codes like "FF" (full frame), "FF_AA1" (alt art 1), etc.

10. RARITY ENCODING
    In the Cards table: 1=land(?), 2=common, 3=uncommon, 4=rare, 5=mythic.
    Verify against Enums table if exact values matter.
"""
