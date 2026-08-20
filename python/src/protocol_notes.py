"""MTGA network protocol reference — extracted from decompiled game assemblies.

This file contains no executable code. It documents the communication architecture
discovered by decompiling Core.dll, SharedClientCore.dll, and Wizards.Arena.Models.dll.

Architecture
============
MTGA uses a TCP connection (FrontDoorConnectionAWS) that wraps messages in protobuf
envelopes (Wizards.Arena.Protocol.Cmd). Payloads are JSON serialized inside the
protobuf wrapper. Responses are protobuf-wrapped JSON with gzip compression.

All requests go through FrontDoorConnectionAWS.SendMessage<T>(CmdType, request).

Logging
=======
Only messages with CmdType in the LoggableMessages whitelist are written to Player.log.
The whitelist (28 commands) is defined in FrontDoorConnectionAWS and includes:

    DeckUpsertDeckV3, DeckGetDeck, DeckGetDeckSummariesV3, DeckGetPreconDeckV2,
    DeckGetAllPreconDecksV3, DeckDeleteDeck, DcIssueChallenge, BotDraftDraftStatus,
    BotDraftDraftPick, EventPlayerDraftMakePick, EventAiBotMatch, EventClaimPrize,
    DraftCompleteDraft, EventSetDeckV3, EventGetActiveEvents, EventGetActiveMatches,
    RankGetCombinedRankInfo, EventGetCoursesV2, RankGetSeasonAndRankDetails,
    EventJoin, EventEnterPairing, LogBusinessEvents, GetFormats, StartHook,
    PeriodicRewardsGetStatus, QuestGetQuests, GraphProcess, GraphGetGraphState

NOT logged (relevant to us):
    CardRedeemWildCards (552)  — crafting/wildcard redemption
    CardGetAllCards (551)      — full collection fetch
    CardGetCardSet (550)       — per-set card fetch
    CurrencyGetCurrencies (800)
    BoosterOpenBoosters (900)  — pack opening (response IS logged via push notification)
    BoosterGetOwnedBoosters (901)

Push Notifications
==================
Server-initiated events are delivered as PushNotification messages with an
ENotificationType enum:

    MatchCreated        — match assignment
    InventoryChanged    — inventory state update (happens after crafting, pack opens, etc.)
    SystemMessage       — maintenance notices, etc.
    DraftNotification   — draft events
    KillSwitchNotification — feature flags
    FriendRequestNotification
    CampaignGraphDeltaNotification — mastery/battle pass progress
    LobbyNotification   — direct challenge lobbies
    TournamentNotification

The InventoryChanged push notification IS logged to Player.log because the game
processes it through OnMsg_InventoryInfoUpdated_AWS which updates the client state.
This is the "InventoryUpdate" event we see in watch.py when rewards are granted.

Crafting Flow
=============
1. UI calls InventoryManager.OnCraftClicked → RedeemWildcards
2. AwsInventoryServiceWrapper.RedeemWildcards(WildcardBulkRequest) is called
3. Request is converted via AWSInventoryConversions.ToAwsWildCardBulkRequest
4. Sent as CmdType.CardRedeemWildCards (552)
5. Response is InventoryInfo — processed via OnInventoryInfoUpdated_AWS
6. The InventoryInfo includes Changes[] with Source=WildCardRedemption

The request itself (CmdType 552) is NOT logged because it's not in LoggableMessages.
The RESPONSE triggers an InventoryChanged push notification which IS visible.
However, by the time we see it in the log, the crafting is complete and the
WildCardRedemption source is inside Changes[].Source — but the log format for
these push notifications doesn't match the ==> / <== API call format that we
parse as "InventoryUpdate" events. The push notification InventoryInfo is what
triggers the log_watcher "InventoryUpdate" classification.

WildcardBulkRequest structure:
    { bulkRequest: [{cardId: int, quantity: int}, ...] }

Inventory Fields (ClientPlayerInventory)
========================================
    gold: int
    gems: int
    wcCommon: int
    wcUncommon: int
    wcRare: int
    wcMythic: int
    wcTrackPosition: int
    vaultProgress: float  (TotalVaultProgress / 10.0)
    boosters: [{collationId, count}, ...]
    vouchers: [{referenceId, count}, ...]
    tickets: [{...}, ...]
    customTokens: {str: int}  — includes DraftToken, SealedToken
    basicLandSet: str
    starterDecks: guid[]

Inventory Change Sources (InventoryUpdateSource enum)
=====================================================
    QuestReward, DailyWins, WeeklyWins, LoginGrant,
    BattlePassLevelUp, BattlePassLevelMasteryTree,
    MercantilePurchase, MercantileBoosterPurchase,
    EventReward, RedeemVoucher, OpenChest, MassOpenChest,
    CompleteVault, WildCardRedemption, BoosterOpen,
    StarterDeckUpgrade, RankedSeasonReward, EventPayEntry,
    BannedCardGrant, CatalogPurchase, CampaignGraphReward,
    Letter, CrossPlatformReward, ...

CmdType Enum (complete, from ECmdType)
======================================
    Authenticate=0, StartHook=1, Attach=5, GetFormats=6,
    Card_GetCardSet=550, Card_GetAllCards=551, Card_RedeemWildCards=552,
    Card_CompleteVault=553,
    Event_Join=600, Event_Drop=601, Event_EnterPairing=603,
    Event_GetActiveEvents=604, Event_LeavePairing=606, Event_ClaimPrize=607,
    Event_Resign=609, Event_AiBotMatch=612, Event_GetActiveMatches=613,
    Currency_GetCurrencies=800,
    Booster_OpenBoosters=900, Booster_GetOwnedBoosters=901,
    Quest_GetQuests=1000, Quest_SwapQuest=1001,
    Rank_GetCombinedRankInfo=1100, Rank_GetSeasonAndRankDetails=1102,
    PeriodicRewards_GetStatus=1200,
    Account_SetAvatar=1603,
    Graph_GetGraphState=1701, Graph_Process=1702,
    BotDraft_StartDraft=1800, BotDraft_DraftPick=1801, BotDraft_DraftStatus=1802,
    Draft_CompleteDraft=1908,
    LogBusinessEvents=1913, LogBusinessEventsV2=1914,
    GetPlayerInbox=2300, MarkLetterRead=2301, ClaimAttachment=2302,
    Merc_PurchaseV2=707, Merc_GetSkusAndListings=715,
    Store_GetEntitlementsV2=712,
"""
