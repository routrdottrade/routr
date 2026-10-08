use near_sdk::json_types::{Base58CryptoHash, U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::serde_json::{self, json, Value};
use near_sdk::store::LookupMap;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue, PromiseResult,
};
use std::borrow::Cow;
use uint::construct_uint;

mod balances;
pub use balances::*;

construct_uint! {
    pub struct U256(4);
}
construct_uint! {
    pub struct U512(8);
}

pub fn impact_bps(y0: u128, x0: u128, y1: u128, x1: u128) -> u32 {
    let a = U512::from(y1) * U512::from(x0);
    let b = U512::from(y0) * U512::from(x1);
    let num = if a > b { a - b } else { b - a };
    let den = (U512::from(y0) * U512::from(x1)).max(U512::one());
    let r = num * U512::from(BPS) / den;
    if r > U512::from(u32::MAX) {
        u32::MAX
    } else {
        r.as_u32()
    }
}

pub const EVENT_STANDARD: &str = "routr";

pub const EVENT_VERSION: &str = "1.2.0";
const BPS: u128 = 10_000;
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

pub const PROTOCOL_FEE_CAP_BPS: u16 = 3_000;

pub const POOL_FEE_CAP_BPS: u16 = 1_000;

pub const VIRTUAL_QUOTE_MAX: u128 = u128::MAX / 8;

pub const RESERVE_MAX: u128 = u128::MAX / 4;

pub const PLATFORM_BOND: NearToken = NearToken::from_millinear(500);

const SWAP_STORAGE_BYTES: u64 = 14 * 200;

const KEY_BYTES: u64 = 200;

const BUILDER_KEYS: u128 = 3;

const BUILDER_KEY_BYTES: u64 = 216;

const POOL_BYTES: u64 = 760;

pub const OPEN_FEE_CAP_BPS: u16 = 5_000;

const GAS_DELIVER_CALL_DEFAULT: Gas = Gas::from_tgas(50);

const GAS_DELIVER_CALL_MIN: Gas = Gas::from_tgas(50);
const GAS_DELIVER_CALL_MAX: Gas = Gas::from_tgas(250);

const DELIVER_MSG_MAX: usize = 2_048;
pub const MAX_BUILDERS: usize = 32;

pub const MAX_HOPS: usize = 3;
const GAS_FT_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_ON_DELIVERED: Gas = Gas::from_tgas(10);

const GAS_MIGRATE: Gas = Gas::from_tgas(50);

const GAS_CHECK_DISPATCH: Gas = Gas::from_tgas(5);

pub const STATE_VERSION: u32 = 1;

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct PlatformV1 {
    pub id: String,
    pub owner: AccountId,
    pub fee_recipient: AccountId,
    pub creator_bps: u16,
    pub builder_bps: u16,
    pub pool_creators: Vec<AccountId>,
    pub storage_credit: U128,
    pub created_ms: U64,
    pub pools: u64,
    pub builders: Vec<AccountId>,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug)]
pub struct Platform {
    pub id: String,
    pub owner: AccountId,
    pub fee_recipient: AccountId,

    pub creator_bps: u16,

    pub builder_bps: u16,

    pub pool_creators: Vec<AccountId>,

    pub storage_credit: U128,
    pub created_ms: U64,
    pub pools: u64,

    pub builders: Vec<AccountId>,

    pub open_builder_bps: u16,
}

impl From<PlatformV1> for Platform {
    fn from(p: PlatformV1) -> Self {
        Platform {
            id: p.id,
            owner: p.owner,
            fee_recipient: p.fee_recipient,
            creator_bps: p.creator_bps,
            builder_bps: p.builder_bps,
            pool_creators: p.pool_creators,
            storage_credit: p.storage_credit,
            created_ms: p.created_ms,
            pools: p.pools,
            builders: p.builders,
            open_builder_bps: 0,
        }
    }
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub enum VPlatform {
    V1(PlatformV1),
    V2(Platform),
}

impl VPlatform {

    fn get(&self) -> Cow<'_, Platform> {
        match self {
            VPlatform::V1(p) => Cow::Owned(p.clone().into()),
            VPlatform::V2(p) => Cow::Borrowed(p),
        }
    }

    fn get_mut(&mut self) -> &mut Platform {
        if let VPlatform::V1(p) = self {
            *self = VPlatform::V2(p.clone().into());
        }
        match self {
            VPlatform::V2(p) => p,
            VPlatform::V1(_) => env::panic_str("E_RECORD_VERSION"),
        }
    }
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct PoolTermsV1 {
    pub protocol_fee_bps: u16,
    pub protocol_recipient: AccountId,
    pub platform_recipient: AccountId,
    pub creator_bps: u16,
    pub builder_bps: u16,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeeSchedule {
    pub start_bps: u16,
    pub end_bps: u16,
    pub duration_ms: U64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolTerms {
    pub protocol_fee_bps: u16,
    pub protocol_recipient: AccountId,
    pub platform_recipient: AccountId,
    pub creator_bps: u16,
    pub builder_bps: u16,

    pub open_builder_bps: u16,

    pub fee_schedule: Option<FeeSchedule>,
}

impl From<PoolTermsV1> for PoolTerms {
    fn from(t: PoolTermsV1) -> Self {
        PoolTerms {
            protocol_fee_bps: t.protocol_fee_bps,
            protocol_recipient: t.protocol_recipient,
            platform_recipient: t.platform_recipient,
            creator_bps: t.creator_bps,
            builder_bps: t.builder_bps,
            open_builder_bps: 0,
            fee_schedule: None,
        }
    }
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct PoolV1 {
    pub id: U64,
    pub token: AccountId,
    pub quote: AccountId,
    pub platform_id: String,
    pub creator: AccountId,
    pub seeder: AccountId,
    pub fee_bps: u16,
    pub seeded: bool,
    pub initial_token: U128,
    pub real_token: U128,
    pub real_quote: U128,
    pub virtual_quote: U128,
    pub volume_quote: U128,
    pub fees_quote: U128,
    pub fees_protocol: U128,
    pub fees_platform: U128,
    pub fees_creator: U128,
    pub fees_builder: U128,
    pub swaps: u64,
    pub created_ms: U64,
    pub terms: PoolTermsV1,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    pub id: U64,
    pub token: AccountId,
    pub quote: AccountId,
    pub platform_id: String,
    pub creator: AccountId,

    pub seeder: AccountId,

    pub fee_bps: u16,
    pub seeded: bool,
    pub initial_token: U128,
    pub real_token: U128,
    pub real_quote: U128,

    pub virtual_quote: U128,
    pub volume_quote: U128,
    pub fees_quote: U128,

    pub fees_protocol: U128,
    pub fees_platform: U128,
    pub fees_creator: U128,

    pub fees_builder: U128,

    pub fees_open_builder: U128,
    pub swaps: u64,
    pub created_ms: U64,

    pub seeded_ms: U64,

    pub terms: PoolTerms,
}

impl From<PoolV1> for Pool {
    fn from(p: PoolV1) -> Self {
        Pool {
            id: p.id,
            token: p.token,
            quote: p.quote,
            platform_id: p.platform_id,
            creator: p.creator,
            seeder: p.seeder,
            fee_bps: p.fee_bps,
            seeded: p.seeded,
            initial_token: p.initial_token,
            real_token: p.real_token,
            real_quote: p.real_quote,
            virtual_quote: p.virtual_quote,
            volume_quote: p.volume_quote,
            fees_quote: p.fees_quote,
            fees_protocol: p.fees_protocol,
            fees_platform: p.fees_platform,
            fees_creator: p.fees_creator,
            fees_builder: p.fees_builder,
            fees_open_builder: U128(0),
            swaps: p.swaps,
            created_ms: p.created_ms,
            seeded_ms: p.created_ms,
            terms: p.terms.into(),
        }
    }
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub enum VPool {
    V1(PoolV1),
    V2(Pool),
}

impl VPool {

    fn get(&self) -> Cow<'_, Pool> {
        match self {
            VPool::V1(p) => Cow::Owned(p.clone().into()),
            VPool::V2(p) => Cow::Borrowed(p),
        }
    }

    fn get_mut(&mut self) -> &mut Pool {
        if let VPool::V1(p) = self {
            *self = VPool::V2(p.clone().into());
        }
        match self {
            VPool::V2(p) => p,
            VPool::V1(_) => env::panic_str("E_RECORD_VERSION"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BuilderTier {
    None,
    Approved,
    Open,
}

impl BuilderTier {
    fn name(self) -> Option<&'static str> {
        match self {
            BuilderTier::None => None,
            BuilderTier::Approved => Some("approved"),
            BuilderTier::Open => Some("open"),
        }
    }
}

struct Delivery {
    msg: String,
    gas: Gas,
}

#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct SwapQuote {
    pub amount_in: U128,
    pub amount_out: U128,
    pub fee: U128,
    pub fee_protocol: U128,
    pub fee_platform: U128,
    pub fee_creator: U128,

    pub fee_builder: U128,

    pub fee_open_builder: U128,

    pub price_before: [U128; 2],
    pub price_after: [U128; 2],
    pub price_impact_bps: u32,

    pub fee_bps_applied: u16,

    pub builder_tier: Option<String>,

    pub executable: bool,
    pub reason: Option<String>,
}

impl SwapQuote {
    fn refused(amount_in: U128, amount_out: U128, why: &str) -> SwapQuote {
        SwapQuote {
            amount_in,
            amount_out,
            fee: U128(0),
            fee_protocol: U128(0),
            fee_platform: U128(0),
            fee_creator: U128(0),
            fee_builder: U128(0),
            fee_open_builder: U128(0),
            price_before: [U128(0), U128(0)],
            price_after: [U128(0), U128(0)],
            price_impact_bps: 0,
            fee_bps_applied: 0,
            builder_tier: None,
            executable: false,
            reason: Some(why.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum Msg {
    Seed {

        pool_id: String,

        expect_creator: Option<AccountId>,
    },
    Swap {

        pool_id: String,
        min_out: U128,

        recipient: Option<AccountId>,

        builder: Option<AccountId>,

        deliver_msg: Option<String>,

        deliver_gas: Option<U64>,
    },

    Route {

        pool_ids: Vec<String>,

        min_out: U128,
        recipient: Option<AccountId>,
        builder: Option<AccountId>,

        deliver_msg: Option<String>,
        deliver_gas: Option<U64>,
    },

    Deposit {},

    PlaceOrder {
        pool_id: String,
        min_out: U128,
        expires_ms: U64,
        tip: U128,
        builder: Option<AccountId>,
    },
}

#[near(serializers = [borsh])]
#[derive(BorshStorageKey)]
enum StorageKey {
    Platforms,
    Pools,
    PoolIds,
    Owed,
    OwedTotal,
    Pending,
    Delivered,
    PlatformPools,
}

#[near(serializers = [borsh])]
pub struct RoutrV0 {
    owner: AccountId,
    protocol_fee_bps: u16,
    protocol_recipient: AccountId,
    platforms: LookupMap<String, VPlatform>,
    pools: LookupMap<u64, VPool>,
    pool_ids: LookupMap<String, u64>,
    platform_pools: LookupMap<(String, u64), u64>,
    next_pool: u64,
    owed: LookupMap<(AccountId, AccountId), u128>,
    pending: LookupMap<(AccountId, AccountId), u128>,
    owed_total: LookupMap<AccountId, u128>,
    delivered: LookupMap<(AccountId, AccountId), u128>,
    inflight: u64,
}

#[near(
    contract_state,
    contract_metadata(
        version = "0.4.0",
        link = "https://github.com/routrdottrade/routr",
        standard(standard = "nep297", version = "1.0.0"),
    )
)]
#[derive(PanicOnDefault)]
pub struct Routr {
    owner: AccountId,
    protocol_fee_bps: u16,
    protocol_recipient: AccountId,
    platforms: LookupMap<String, VPlatform>,
    pools: LookupMap<u64, VPool>,
    pool_ids: LookupMap<String, u64>,

    platform_pools: LookupMap<(String, u64), u64>,
    next_pool: u64,

    owed: LookupMap<(AccountId, AccountId), u128>,

    pending: LookupMap<(AccountId, AccountId), u128>,

    owed_total: LookupMap<AccountId, u128>,

    delivered: LookupMap<(AccountId, AccountId), u128>,

    inflight: u64,

    guardian: Option<AccountId>,
    paused: bool,

    upgrade_delay_ms: u64,

    upgrade_hash: [u8; 32],
    upgrade_ready_ms: u64,

    unpause_ready_ms: u64,

    pending_owner: Option<AccountId>,
    state_version: u32,
}

fn emit(event: &str, data: Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({"standard": EVENT_STANDARD, "version": EVENT_VERSION, "event": event, "data": [data]})
    ));
}

fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    let r = U256::from(a) * U256::from(b) / U256::from(c);
    r.as_u128()
}

pub fn share(amount: u128, bps: u16) -> u128 {
    mul_div(amount, bps as u128, BPS)
}

pub fn pool_key(token: &AccountId, quote: &AccountId, platform_id: &str) -> String {
    format!("{token}|{quote}|{platform_id}")
}

pub fn valid_platform_id(s: &str) -> bool {
    (2..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn cp_out(amount_in: u128, reserve_in: u128, reserve_out: u128) -> u128 {
    if amount_in == 0 || reserve_out == 0 {
        return 0;
    }
    let num = U256::from(reserve_out) * U256::from(amount_in);
    let den = U256::from(reserve_in) + U256::from(amount_in);
    (num / den).as_u128()
}

pub fn cp_in(amount_out: u128, reserve_in: u128, reserve_out: u128) -> Option<u128> {
    if amount_out == 0 {
        return Some(0);
    }
    if amount_out >= reserve_out {
        return None;
    }
    let num = U256::from(reserve_in) * U256::from(amount_out);
    let den = U256::from(reserve_out - amount_out);
    let q = (num + den - U256::one()) / den;
    (q <= U256::from(u128::MAX)).then(|| q.as_u128())
}

pub fn gross_up(a: u128, bps: u16) -> Option<u128> {
    if a == 0 { return Some(0); }
    if bps as u128 >= BPS { return None; }
    let den = U256::from(BPS - bps as u128);
    let q = U256::from(a - 1) * U256::from(BPS) / den + U256::one();
    (q <= U256::from(u128::MAX)).then(|| q.as_u128())
}

pub fn fee_bps_at(fee_bps: u16, schedule: &Option<FeeSchedule>, seeded_ms: u64, now_ms: u64) -> u16 {
    let Some(s) = schedule else { return fee_bps };
    if seeded_ms == 0 {
        return fee_bps;
    }
    let t = now_ms.saturating_sub(seeded_ms);
    if t >= s.duration_ms.0 {
        return fee_bps;
    }
    let drop = (s.start_bps as u128 - s.end_bps as u128) * t as u128 / s.duration_ms.0 as u128;
    s.start_bps - drop as u16
}

#[near]
impl Routr {

    #[init]
    pub fn new(
        owner: AccountId,
        protocol_fee_bps: u16,
        protocol_recipient: AccountId,
        guardian: Option<AccountId>,
        upgrade_delay_ms: U64,
    ) -> Self {
        require!(protocol_fee_bps <= PROTOCOL_FEE_CAP_BPS, "E_PROTOCOL_FEE");
        require!(upgrade_delay_ms.0 > 0, "E_DELAY");
        Self {
            owner,
            protocol_fee_bps,
            protocol_recipient,
            platforms: LookupMap::new(StorageKey::Platforms),
            pools: LookupMap::new(StorageKey::Pools),
            pool_ids: LookupMap::new(StorageKey::PoolIds),
            platform_pools: LookupMap::new(StorageKey::PlatformPools),
            next_pool: 1,
            owed: LookupMap::new(StorageKey::Owed),
            pending: LookupMap::new(StorageKey::Pending),
            owed_total: LookupMap::new(StorageKey::OwedTotal),
            delivered: LookupMap::new(StorageKey::Delivered),
            inflight: 0,
            guardian,
            paused: false,
            upgrade_delay_ms: upgrade_delay_ms.0,
            upgrade_hash: [0; 32],
            upgrade_ready_ms: 0,
            unpause_ready_ms: 0,
            pending_owner: None,
            state_version: STATE_VERSION,
        }
    }

    #[private]
    #[init(ignore_state)]
    pub fn migrate(guardian: Option<AccountId>, upgrade_delay_ms: Option<U64>) -> Self {
        let raw = env::storage_read(b"STATE").unwrap_or_else(|| env::panic_str("E_NO_STATE"));
        if let Ok(mut me) = near_sdk::borsh::from_slice::<Self>(&raw) {
            require!(me.state_version <= STATE_VERSION, "E_STATE_FROM_THE_FUTURE");
            me.state_version = STATE_VERSION;

            if me.upgrade_ready_ms != 0 {
                emit("upgrade_landed", json!({"code_hash": Base58CryptoHash::from(me.upgrade_hash)}));
                me.upgrade_hash = [0; 32];
                me.upgrade_ready_ms = 0;
            }
            return me;
        }
        let old: RoutrV0 =
            near_sdk::borsh::from_slice(&raw).unwrap_or_else(|_| env::panic_str("E_UNKNOWN_STATE_LAYOUT"));
        let delay = upgrade_delay_ms.unwrap_or_else(|| env::panic_str("E_DELAY_REQUIRED"));
        require!(delay.0 > 0, "E_DELAY");
        Self {
            owner: old.owner,
            protocol_fee_bps: old.protocol_fee_bps,
            protocol_recipient: old.protocol_recipient,
            platforms: old.platforms,
            pools: old.pools,
            pool_ids: old.pool_ids,
            platform_pools: old.platform_pools,
            next_pool: old.next_pool,
            owed: old.owed,
            pending: old.pending,
            owed_total: old.owed_total,
            delivered: old.delivered,
            inflight: old.inflight,
            guardian,
            paused: false,
            upgrade_delay_ms: delay.0,
            upgrade_hash: [0; 32],
            upgrade_ready_ms: 0,
            unpause_ready_ms: 0,
            pending_owner: None,
            state_version: STATE_VERSION,
        }
    }

    #[payable]
    pub fn register_platform(
        &mut self,
        platform_id: String,
        fee_recipient: AccountId,
        creator_bps: u16,
        builder_bps: u16,
        pool_creators: Option<Vec<AccountId>>,
        builders: Option<Vec<AccountId>>,
        open_builder_bps: Option<u16>,
    ) {
        self.require_live();
        require!(valid_platform_id(&platform_id), "E_PLATFORM_ID");
        require!(!self.platforms.contains_key(&platform_id), "E_PLATFORM_EXISTS");
        let open_builder_bps = open_builder_bps.unwrap_or(0);
        require!(creator_bps as u128 <= BPS && builder_bps as u128 <= BPS, "E_BPS");
        require!(creator_bps as u128 + builder_bps as u128 <= BPS, "E_BPS");
        require!(creator_bps as u128 + open_builder_bps as u128 <= BPS, "E_BPS");
        let pool_creators = pool_creators.unwrap_or_default();
        require!(pool_creators.len() <= 8, "E_TOO_MANY_CREATORS");
        let builders = builders.unwrap_or_default();
        require!(builders.len() <= MAX_BUILDERS, "E_TOO_MANY_BUILDERS");
        let before = env::storage_usage();
        let owner = env::predecessor_account_id();
        self.platforms.insert(
            platform_id.clone(),
            VPlatform::V2(Platform {
                id: platform_id.clone(),
                owner: owner.clone(),
                fee_recipient: fee_recipient.clone(),
                creator_bps,
                builder_bps,
                pool_creators: pool_creators.clone(),
                storage_credit: U128(PLATFORM_BOND.as_yoctonear()),
                created_ms: U64(env::block_timestamp_ms()),
                pools: 0,
                builders: builders.clone(),
                open_builder_bps,
            }),
        );
        self.platforms.flush();
        Self::settle_storage(before, PLATFORM_BOND.as_yoctonear());
        emit(
            "platform_registered",
            json!({"platform_id": platform_id, "owner": owner, "fee_recipient": fee_recipient,
                "creator_bps": creator_bps, "builder_bps": builder_bps, "open_builder_bps": open_builder_bps,
                "pool_creators": pool_creators, "builders": builders}),
        );
    }

    #[payable]
    pub fn set_platform(
        &mut self,
        platform_id: String,
        fee_recipient: Option<AccountId>,
        creator_bps: Option<u16>,
        builder_bps: Option<u16>,
        owner: Option<AccountId>,
        pool_creators: Option<Vec<AccountId>>,
        builders: Option<Vec<AccountId>>,
        open_builder_bps: Option<u16>,
    ) {
        self.require_live();
        let p = self
            .platforms
            .get_mut(&platform_id)
            .unwrap_or_else(|| env::panic_str("E_NO_PLATFORM"))
            .get_mut();
        require!(env::predecessor_account_id() == p.owner, "E_PLATFORM_OWNER");
        let before = env::storage_usage();
        if let Some(r) = fee_recipient {
            p.fee_recipient = r;
        }
        if let Some(c) = pool_creators {
            require!(c.len() <= 8, "E_TOO_MANY_CREATORS");
            p.pool_creators = c;
        }
        if let Some(b) = builders {
            require!(b.len() <= MAX_BUILDERS, "E_TOO_MANY_BUILDERS");
            p.builders = b;
        }
        if let Some(c) = creator_bps {
            p.creator_bps = c;
        }
        if let Some(b) = builder_bps {
            p.builder_bps = b;
        }
        if let Some(o) = open_builder_bps {
            p.open_builder_bps = o;
        }
        require!(p.creator_bps as u128 + p.builder_bps as u128 <= BPS, "E_BPS");
        require!(p.creator_bps as u128 + p.open_builder_bps as u128 <= BPS, "E_BPS");
        if let Some(o) = owner {
            p.owner = o;
        }
        let snapshot = p.clone();
        self.platforms.flush();
        Self::settle_storage(before, 0);
        emit("platform_changed", json!(snapshot));
    }

    #[payable]
    pub fn create_pool(
        &mut self,
        token: AccountId,
        quote: AccountId,
        platform_id: String,
        creator: AccountId,
        seeder: Option<AccountId>,
        fee_bps: u16,
        virtual_quote: U128,
        fee_schedule: Option<FeeSchedule>,
    ) -> U64 {
        self.require_live();
        require!(token != quote, "E_SAME_TOKEN");
        require!(fee_bps > 0 && fee_bps <= POOL_FEE_CAP_BPS, "E_FEE");
        if let Some(sch) = &fee_schedule {
            require!(sch.start_bps <= OPEN_FEE_CAP_BPS, "E_FEE_SCHEDULE");
            require!(sch.end_bps <= sch.start_bps && sch.end_bps >= fee_bps, "E_FEE_SCHEDULE");
            require!(sch.duration_ms.0 > 0, "E_FEE_SCHEDULE");
        }
        require!(virtual_quote.0 > 0 && virtual_quote.0 <= VIRTUAL_QUOTE_MAX, "E_VIRTUAL_QUOTE");
        let key = pool_key(&token, &quote, &platform_id);
        require!(!self.pool_ids.contains_key(&key), "E_POOL_EXISTS");
        let (terms, index) = {
            let p = self
                .platforms
                .get_mut(&platform_id)
                .unwrap_or_else(|| env::panic_str("E_NO_PLATFORM"))
                .get_mut();

            require!(
                p.pool_creators.is_empty() || p.pool_creators.contains(&env::predecessor_account_id()),
                "E_NOT_A_POOL_CREATOR"
            );
            let index = p.pools;
            p.pools += 1;
            (
                PoolTerms {
                    protocol_fee_bps: self.protocol_fee_bps,
                    protocol_recipient: self.protocol_recipient.clone(),
                    platform_recipient: p.fee_recipient.clone(),
                    creator_bps: p.creator_bps,
                    builder_bps: p.builder_bps,
                    open_builder_bps: p.open_builder_bps,
                    fee_schedule: fee_schedule.clone(),
                },
                index,
            )
        };
        let before = env::storage_usage();
        let id = self.next_pool;
        self.next_pool += 1;
        let seeder = seeder.unwrap_or_else(env::predecessor_account_id);
        self.platform_pools.insert((platform_id.clone(), index), id);
        self.pools.insert(
            id,
            VPool::V2(Pool {
                id: U64(id),
                token: token.clone(),
                quote: quote.clone(),
                platform_id: platform_id.clone(),
                creator: creator.clone(),
                seeder: seeder.clone(),
                fee_bps,
                seeded: false,
                initial_token: U128(0),
                real_token: U128(0),
                real_quote: U128(0),
                virtual_quote,
                volume_quote: U128(0),
                fees_quote: U128(0),
                fees_protocol: U128(0),
                fees_platform: U128(0),
                fees_creator: U128(0),
                fees_builder: U128(0),
                fees_open_builder: U128(0),
                swaps: 0,
                created_ms: U64(env::block_timestamp_ms()),
                seeded_ms: U64(0),
                terms: terms.clone(),
            }),
        );
        self.pool_ids.insert(key, id);
        self.pools.flush();
        self.pool_ids.flush();
        self.platforms.flush();
        self.platform_pools.flush();
        Self::settle_storage(before, 0);
        emit(
            "pool_created",
            json!({"pool_id": U64(id), "token": token, "quote": quote, "platform_id": platform_id,
                "creator": creator, "seeder": seeder, "fee_bps": fee_bps, "virtual_quote": virtual_quote,
                "terms": terms}),
        );
        U64(id)
    }

    pub fn drop_pool(&mut self, pool_id: U64) {
        self.require_live();
        let p = self
            .pools
            .get(&pool_id.0)
            .map(|v| v.get().into_owned())
            .unwrap_or_else(|| env::panic_str("E_NO_POOL"));
        require!(!p.seeded, "E_SEEDED");
        let who = env::predecessor_account_id();
        let platform_owner = self
            .platforms
            .get(&p.platform_id)
            .map(|x| x.get().owner.clone())
            .unwrap_or_else(|| env::panic_str("E_NO_PLATFORM"));
        require!(who == platform_owner || who == p.seeder, "E_NOT_ALLOWED");

        self.pools.remove(&pool_id.0);
        self.pool_ids
            .remove(&pool_key(&p.token, &p.quote, &p.platform_id));
        emit("pool_dropped", json!({"pool_id": pool_id, "by": who}));
    }

    #[payable]
    pub fn set_pool_creator(&mut self, pool_id: U64, creator: AccountId) {
        self.require_live();
        let p = self
            .pools
            .get_mut(&pool_id.0)
            .unwrap_or_else(|| env::panic_str("E_NO_POOL"))
            .get_mut();
        require!(env::predecessor_account_id() == p.creator, "E_NOT_CREATOR");
        let before = env::storage_usage();
        p.creator = creator.clone();
        self.pools.flush();
        Self::settle_storage(before, 0);
        emit("pool_creator_changed", json!({"pool_id": pool_id, "creator": creator}));
    }

    #[payable]
    pub fn platform_top_up(&mut self, platform_id: String) {
        self.require_live();
        let p = self
            .platforms
            .get_mut(&platform_id)
            .unwrap_or_else(|| env::panic_str("E_NO_PLATFORM"))
            .get_mut();
        p.storage_credit = U128(p.storage_credit.0 + env::attached_deposit().as_yoctonear());
        emit(
            "platform_topped_up",
            json!({"platform_id": platform_id, "by": env::predecessor_account_id(),
                "amount": U128(env::attached_deposit().as_yoctonear()), "credit": p.storage_credit}),
        );
    }

    #[payable]
    pub fn register_builder(&mut self) {
        self.require_live();
        let who = env::predecessor_account_id();
        require!(who != env::current_account_id(), "E_BUILDER");
        require!(Self::builder_credit(who.as_str()).is_none(), "E_BUILDER_EXISTS");
        let before = env::storage_usage();
        Self::builder_credit_write(who.as_str(), 0);
        let byte = env::storage_byte_cost().as_yoctonear();
        let record = byte * env::storage_usage().saturating_sub(before) as u128;
        let paid = env::attached_deposit().as_yoctonear();
        require!(paid >= record + byte * BUILDER_KEY_BYTES as u128 * BUILDER_KEYS, "E_STORAGE_DEPOSIT");
        let credit = paid - record;
        Self::builder_credit_write(who.as_str(), credit);
        emit("builder_registered", json!({"builder": who, "credit": U128(credit)}));
    }

    #[payable]
    pub fn builder_top_up(&mut self, builder: AccountId) {
        self.require_live();
        let c = Self::builder_credit(builder.as_str()).unwrap_or_else(|| env::panic_str("E_NO_BUILDER"));
        let c = c.saturating_add(env::attached_deposit().as_yoctonear());
        Self::builder_credit_write(builder.as_str(), c);
        emit(
            "builder_topped_up",
            json!({"builder": builder, "by": env::predecessor_account_id(),
                "amount": U128(env::attached_deposit().as_yoctonear()), "credit": U128(c)}),
        );
    }

    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        let token_in = env::predecessor_account_id();
        if self.paused {

            emit("refused_paused", json!({"token": token_in, "sender": sender_id, "amount": amount}));
            return PromiseOrValue::Value(amount);
        }
        if msg == "Deposit" || msg == "\"Deposit\"" {
            return self.deposit(token_in, sender_id, amount);
        }
        let m: Msg = serde_json::from_str(&msg).unwrap_or_else(|_| env::panic_str("E_MSG"));
        match m {
            Msg::Deposit {} => self.deposit(token_in, sender_id, amount),
            Msg::PlaceOrder {
                pool_id,
                min_out,
                expires_ms,
                tip,
                builder,
            } => self.place_sell(token_in, sender_id, amount, pool_id, min_out, expires_ms, tip, builder),
            Msg::Seed {
                pool_id,
                expect_creator,
            } => {
                let id = self.resolve_pool(&pool_id);
                let p = self
                    .pools
                    .get_mut(&id)
                    .unwrap_or_else(|| env::panic_str("E_NO_POOL"))
                    .get_mut();
                require!(!p.seeded, "E_SEEDED");
                require!(token_in == p.token, "E_NOT_THE_TOKEN");
                require!(sender_id == p.seeder, "E_NOT_THE_SEEDER");
                if let Some(c) = expect_creator {
                    require!(p.creator == c, "E_CREATOR_MISMATCH");
                }
                require!(amount.0 > 0, "E_AMOUNT");
                p.seeded = true;
                p.initial_token = amount;
                p.real_token = amount;
                p.seeded_ms = U64(env::block_timestamp_ms());
                let snapshot = p.clone();
                emit(
                    "pool_seeded",
                    json!({"pool_id": U64(id), "token": snapshot.token, "quote": snapshot.quote, "amount": amount,
                        "virtual_quote": snapshot.virtual_quote, "platform_id": snapshot.platform_id,
                        "seeded_ms": snapshot.seeded_ms, "fee_schedule": snapshot.terms.fee_schedule}),
                );
                PromiseOrValue::Value(U128(0))
            }
            Msg::Swap {
                pool_id,
                min_out,
                recipient,
                builder,
                deliver_msg,
                deliver_gas,
            } => {
                let recipient = recipient.unwrap_or(sender_id.clone());

                require!(recipient != env::current_account_id(), "E_RECIPIENT");
                if let Some(b) = &builder {
                    require!(*b != env::current_account_id(), "E_BUILDER");
                }
                let delivery = Self::delivery(deliver_msg, deliver_gas);
                let id = self.resolve_pool(&pool_id);
                self.swap(id, token_in, sender_id, recipient, amount.0, min_out.0, builder, delivery)
            }
            Msg::Route {
                pool_ids,
                min_out,
                recipient,
                builder,
                deliver_msg,
                deliver_gas,
            } => {
                let recipient = recipient.unwrap_or(sender_id.clone());
                require!(recipient != env::current_account_id(), "E_RECIPIENT");
                if let Some(b) = &builder {
                    require!(*b != env::current_account_id(), "E_BUILDER");
                }
                let delivery = Self::delivery(deliver_msg, deliver_gas);
                require!((2..=MAX_HOPS).contains(&pool_ids.len()), "E_ROUTE_LEN");
                let ids: Vec<u64> = pool_ids.iter().map(|r| self.resolve_pool(r)).collect();
                for (i, id) in ids.iter().enumerate() {
                    require!(!ids[..i].contains(id), "E_ROUTE_REPEAT");
                }
                self.route(ids, token_in, sender_id, recipient, amount.0, min_out.0, builder, delivery)
            }
        }
    }

    pub fn push(&mut self, account: AccountId, token: AccountId) -> PromiseOrValue<bool> {
        self.require_live();
        let key = (account.clone(), token.clone());
        let amt = self.owed.remove(&key).unwrap_or(0);
        if amt == 0 {
            return PromiseOrValue::Value(false);
        }

        let payer = Self::owed_payer_take(&key).unwrap_or_default();
        self.refund_key(&payer);
        self.refund_key(&payer);

        let payer = payer.strip_prefix('@').map_or(payer.clone(), |b| format!("#{b}"));
        if self.add_pending(&key, amt) && payer.starts_with('#') {

            Self::pending_payer_set(&key, &payer);
            self.charge_key(&payer);
            self.charge_key(&payer);
        }
        PromiseOrValue::Promise(self.deliver(&token, &account, amt, "push", &payer, None))
    }

    pub fn withdraw(&mut self, token: AccountId) -> PromiseOrValue<bool> {
        self.push(env::predecessor_account_id(), token)
    }

    #[private]
    #[allow(deprecated)]
    pub fn on_delivered(
        &mut self,
        token: AccountId,
        to: AccountId,
        amount: U128,
        why: String,
        platform_id: String,
        refund_to: Option<AccountId>,
    ) -> bool {
        let (ok, used) = match env::promise_result(0) {
            PromiseResult::Successful(v) => {
                let used = match &refund_to {
                    Some(_) => serde_json::from_slice::<U128>(&v).map_or(amount.0, |u| u.0.min(amount.0)),
                    None => amount.0,
                };
                (true, used)
            }
            _ => (false, 0),
        };
        self.inflight = self.inflight.saturating_sub(1);
        let key = (to.clone(), token.clone());
        let p = self.pending.get(&key).copied().unwrap_or(0);
        if p < amount.0 {
            emit("invariant_broken", json!({"what": "pending", "account": to, "token": token, "have": U128(p), "need": amount}));
        }
        let left = p.saturating_sub(amount.0);
        if left == 0 {

            self.pending.remove(&key);
            if let Some(payer) = Self::pending_payer_take(&key) {
                self.refund_key(&payer);
                self.refund_key(&payer);
            }
        } else {
            self.pending.insert(key.clone(), left);
        }
        let refund = amount.0 - used;
        if ok {
            let t = self.owed_total.get(&token).copied().unwrap_or(0);
            if t < used {
                emit("invariant_broken", json!({"what": "owed_total", "token": token, "have": U128(t), "need": U128(used)}));
            }
            self.owed_total.insert(token.clone(), t.saturating_sub(used));
            if used > 0 {
                let d = self.delivered.get(&key).copied();
                self.delivered.insert(key.clone(), d.unwrap_or(0).saturating_add(used));
                if d.is_none() {
                    self.charge_key(&platform_id);
                }
            }
            if refund > 0 {

                let rk = (refund_to.clone().unwrap_or_else(|| to.clone()), token.clone());
                let existing = self.owed.get(&rk).copied();
                self.owed.insert(rk.clone(), existing.unwrap_or(0).saturating_add(refund));
                if existing.is_none() {
                    self.charge_key(&platform_id);
                    self.charge_key(&platform_id);
                    Self::owed_payer_set(&rk, &platform_id);
                }
            }
        } else {

            let existing = self.owed.get(&key).copied();
            self.owed.insert(key.clone(), existing.unwrap_or(0).saturating_add(amount.0));
            if existing.is_none() {
                self.charge_key(&platform_id);
                self.charge_key(&platform_id);
                Self::owed_payer_set(&key, &platform_id);
            }
        }
        let refund_to = (refund > 0).then(|| if ok { refund_to.unwrap_or_else(|| to.clone()) } else { to.clone() });
        emit(
            "payout",
            json!({"token": token, "account": to, "amount": amount, "ok": ok, "why": why,
                "used": U128(used), "refund": U128(refund), "refund_to": refund_to}),
        );
        ok
    }

    #[payable]
    pub fn set_protocol(&mut self, protocol_fee_bps: Option<u16>, protocol_recipient: Option<AccountId>) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        let bytes = protocol_recipient
            .as_ref()
            .map_or(0, |n| (n.len() as u64).saturating_sub(self.protocol_recipient.len() as u64));
        let cost = env::storage_byte_cost().as_yoctonear() * bytes as u128;
        require!(env::attached_deposit().as_yoctonear() >= cost, "E_STORAGE_DEPOSIT");
        if let Some(f) = protocol_fee_bps {
            require!(f <= PROTOCOL_FEE_CAP_BPS, "E_PROTOCOL_FEE");
            self.protocol_fee_bps = f;
        }
        if let Some(r) = protocol_recipient {
            self.protocol_recipient = r;
        }
        emit(
            "protocol_changed",
            json!({"protocol_fee_bps": self.protocol_fee_bps, "protocol_recipient": self.protocol_recipient,
                "owner": self.owner}),
        );
    }

    fn opt_id_bytes(id: &Option<AccountId>) -> u64 {
        1 + id.as_ref().map_or(0, |a| 4 + a.len() as u64)
    }

    fn charge_opt_id_growth(old: &Option<AccountId>, new: &Option<AccountId>) {
        let bytes = Self::opt_id_bytes(new).saturating_sub(Self::opt_id_bytes(old));
        let cost = env::storage_byte_cost().as_yoctonear() * bytes as u128;
        require!(env::attached_deposit().as_yoctonear() >= cost, "E_STORAGE_DEPOSIT");
    }

    #[payable]
    pub fn set_guardian(&mut self, guardian: Option<AccountId>) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        Self::charge_opt_id_growth(&self.guardian, &guardian);
        self.guardian = guardian;
        emit("guardian_changed", json!({"guardian": self.guardian}));
    }

    #[payable]
    pub fn propose_owner(&mut self, owner: Option<AccountId>) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        Self::charge_opt_id_growth(&self.pending_owner, &owner);
        self.pending_owner = owner;
        emit("owner_proposed", json!({"pending_owner": self.pending_owner}));
    }

    pub fn accept_owner(&mut self) {
        let who = env::predecessor_account_id();
        require!(self.pending_owner.as_ref() == Some(&who), "E_NOT_PROPOSED");
        self.owner = who;
        self.pending_owner = None;
        self.upgrade_hash = [0; 32];
        self.upgrade_ready_ms = 0;
        self.unpause_ready_ms = 0;
        emit("owner_changed", json!({"owner": self.owner}));
    }

    pub fn pause(&mut self) {
        let who = env::predecessor_account_id();
        require!(who == self.owner || self.guardian.as_ref() == Some(&who), "E_NOT_GUARDIAN");
        self.paused = true;
        self.unpause_ready_ms = 0;
        emit("paused", json!({"by": who}));
    }

    pub fn stage_unpause(&mut self) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        require!(self.paused, "E_NOT_PAUSED");
        self.unpause_ready_ms = self.ready_at();
        emit("unpause_staged", json!({"ready_ms": U64(self.unpause_ready_ms)}));
    }

    pub fn unpause(&mut self) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        require!(self.unpause_ready_ms != 0, "E_NOT_STAGED");
        require!(env::block_timestamp_ms() >= self.unpause_ready_ms, "E_NOT_YET");
        self.paused = false;
        self.unpause_ready_ms = 0;
        emit("unpaused", json!({}));
    }

    pub fn stage_upgrade(&mut self, code_hash: Base58CryptoHash) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        self.upgrade_hash = code_hash.into();
        self.upgrade_ready_ms = self.ready_at();
        emit("upgrade_staged", json!({"code_hash": code_hash, "ready_ms": U64(self.upgrade_ready_ms)}));
    }

    pub fn cancel_upgrade(&mut self) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        require!(self.upgrade_ready_ms != 0, "E_NOT_STAGED");
        let hash = Base58CryptoHash::from(self.upgrade_hash);
        self.upgrade_hash = [0; 32];
        self.upgrade_ready_ms = 0;
        emit("upgrade_cancelled", json!({"code_hash": hash}));
    }

    fn ready_at(&self) -> u64 {
        env::block_timestamp_ms()
            .checked_add(self.upgrade_delay_ms)
            .unwrap_or_else(|| env::panic_str("E_DELAY_OVERFLOW"))
    }

    #[private]
    pub fn check_dispatch(&self, code_hash: Base58CryptoHash, code_len: U64) {
        let hash: [u8; 32] = code_hash.into();
        require!(self.upgrade_ready_ms != 0 && self.upgrade_hash == hash, "E_STALE_DISPATCH");
        require!(env::block_timestamp_ms() >= self.upgrade_ready_ms, "E_NOT_YET");
        require!(env::account_balance().as_yoctonear() >= self.headroom_for(code_len.0), "E_STORAGE_HEADROOM");
    }

    fn headroom_for(&self, code_len: u64) -> u128 {
        env::storage_byte_cost().as_yoctonear()
            * (env::storage_usage() as u128 + code_len as u128 + 2 * BUILDER_KEY_BYTES as u128 * (self.inflight as u128 + 1))
    }

    pub fn get_protocol(&self) -> Value {
        let staged = (self.upgrade_ready_ms != 0)
            .then(|| json!({"code_hash": Base58CryptoHash::from(self.upgrade_hash), "ready_ms": U64(self.upgrade_ready_ms)}));
        json!({"owner": self.owner, "protocol_fee_bps": self.protocol_fee_bps,
            "protocol_recipient": self.protocol_recipient, "protocol_fee_cap_bps": PROTOCOL_FEE_CAP_BPS,
            "pool_fee_cap_bps": POOL_FEE_CAP_BPS, "pools": self.next_pool - 1,
            "guardian": self.guardian, "paused": self.paused, "upgrade_delay_ms": U64(self.upgrade_delay_ms),
            "staged_upgrade": staged,
            "unpause_ready_ms": (self.unpause_ready_ms != 0).then(|| U64(self.unpause_ready_ms)),
            "pending_owner": self.pending_owner, "state_version": self.state_version})
    }

    pub fn get_platform(&self, platform_id: String) -> Option<Platform> {
        self.platforms.get(&platform_id).map(|v| v.get().into_owned())
    }

    pub fn get_builder(&self, builder: AccountId) -> Option<Value> {
        let credit = Self::builder_credit(builder.as_str())?;
        let key = env::storage_byte_cost().as_yoctonear() * BUILDER_KEY_BYTES as u128;
        Some(json!({"builder": builder, "credit": U128(credit), "keys_covered": credit / key.max(1),
            "earning": credit >= key * BUILDER_KEYS, "keys_needed": BUILDER_KEYS}))
    }

    pub fn get_pool(&self, pool_id: String) -> Option<Pool> {
        let id = self.try_resolve_pool(&pool_id)?;
        self.pools.get(&id).map(|v| v.get().into_owned())
    }

    pub fn get_fee_bps(&self, pool_id: String) -> Option<u16> {
        let id = self.try_resolve_pool(&pool_id)?;
        self.pools.get(&id).map(|v| Self::fee_now(&v.get()))
    }

    pub fn get_platform_pools(&self, platform_id: String, from: u64, limit: u64) -> Vec<Pool> {
        let n = self
            .platforms
            .get(&platform_id)
            .map_or(0, |p| p.get().pools);
        (from..from.saturating_add(limit.min(100)).min(n))
            .filter_map(|i| self.platform_pools.get(&(platform_id.clone(), i)))
            .filter_map(|id| self.pools.get(id).map(|v| v.get().into_owned()))
            .collect()
    }

    pub fn get_launch_status(&self, token: AccountId, quote: AccountId, platform_id: String) -> Value {
        let key = pool_key(&token, &quote, &platform_id);
        let pool = self
            .pool_ids
            .get(&key)
            .and_then(|id| self.pools.get(id).map(|v| v.get().into_owned()));
        let platform = self.platforms.get(&platform_id).map(|v| v.get().into_owned());
        let terms = match (&pool, &platform) {
            (Some(p), _) => Some(p.terms.clone()),
            (None, Some(pl)) => Some(PoolTerms {
                protocol_fee_bps: self.protocol_fee_bps,
                protocol_recipient: self.protocol_recipient.clone(),
                platform_recipient: pl.fee_recipient.clone(),
                creator_bps: pl.creator_bps,
                builder_bps: pl.builder_bps,
                open_builder_bps: pl.open_builder_bps,
                fee_schedule: None,
            }),
            _ => None,
        };
        let byte = env::storage_byte_cost().as_yoctonear();

        let ids = token.len() + quote.len() + platform_id.len()
            + platform.as_ref().map_or(0, |p| p.fee_recipient.len() + 2 * p.owner.len() + self.protocol_recipient.len());
        let est_bytes = POOL_BYTES as usize + 2 * ids + 2 * key.len();
        json!({
            "key": key,
            "state": match &pool { None => "absent", Some(p) if !p.seeded => "created", Some(_) => "seeded" },
            "pool": pool,
            "platform_known": platform.is_some(),
            "terms": terms,
            "create_pool_deposit": U128(byte * (est_bytes as u128) * 12 / 10),
            "create_pool_deposit_note": "an upper bound with margin; create_pool settles the exact cost and refunds the rest",
            "fee_bps_now": pool.as_ref().map(Self::fee_now),
            "swaps_open": !self.paused && pool.as_ref().is_some_and(|p| p.seeded) && self.room_for(&platform_id),
            "swaps_closed_reason": if self.paused { Some("paused") } else if pool.as_ref().is_none_or(|p| !p.seeded) { Some("unseeded") }
                else if !self.room_for(&platform_id) { Some("storage") } else { None },
            "platform_credit": platform.as_ref().map(|p| p.storage_credit),
        })
    }

    pub fn get_pool_id(&self, token: AccountId, quote: AccountId, platform_id: String) -> Option<U64> {
        self.pool_ids
            .get(&pool_key(&token, &quote, &platform_id))
            .map(|i| U64(*i))
    }

    pub fn get_price(&self, pool_id: String) -> Option<[U128; 2]> {
        let id = self.try_resolve_pool(&pool_id)?;
        self.pools.get(&id).map(|v| v.get()).filter(|p| p.seeded).map(|p| {
            [
                U128(p.real_quote.0 + p.virtual_quote.0),
                U128(p.real_token.0),
            ]
        })
    }

    pub fn get_pools(&self, from: U64, limit: u64) -> Vec<Pool> {
        let end = from.0.saturating_add(limit.min(100)).min(self.next_pool);
        (from.0..end)
            .filter_map(|i| self.pools.get(&i).map(|v| v.get().into_owned()))
            .collect()
    }

    pub fn get_storage(&self, platform_id: Option<String>) -> Value {
        let stake = env::storage_byte_cost().as_yoctonear() * env::storage_usage() as u128;
        let free = env::account_balance().as_yoctonear().saturating_sub(stake);
        let per_swap = env::storage_byte_cost().as_yoctonear() * SWAP_STORAGE_BYTES as u128;
        let credit = platform_id
            .and_then(|id| self.platforms.get(&id).map(|p| p.get().storage_credit.0));
        json!({"free": U128(free), "per_swap": U128(per_swap), "swaps_covered": free / per_swap.max(1), "inflight": self.inflight,
            "platform_credit": credit.map(U128), "platform_swaps_covered": credit.map(|c| c / per_swap.max(1))})
    }

    pub fn get_pending(&self, account: AccountId, token: AccountId) -> U128 {
        U128(self.pending.get(&(account, token)).copied().unwrap_or(0))
    }

    pub fn get_delivered(&self, account: AccountId, token: AccountId) -> U128 {
        U128(self.delivered.get(&(account, token)).copied().unwrap_or(0))
    }

    pub fn get_claim_info(&self, pool_id: String, account: AccountId) -> Option<Value> {
        let id = self.try_resolve_pool(&pool_id)?;
        let p = self.pools.get(&id)?.get();
        let d = self.delivered.get(&(account, p.quote.clone())).copied().unwrap_or(0);
        Some(json!({"pool_id": U64(id), "token": p.token, "quote": p.quote, "creator": p.creator,
            "fees_creator": p.fees_creator, "fees_builder": p.fees_builder, "fees_open_builder": p.fees_open_builder,
            "delivered": U128(d)}))
    }

    pub fn quote_swap(&self, pool_id: String, token_in: AccountId, amount_in: U128, builder: Option<AccountId>,
                      sender: Option<AccountId>) -> Option<SwapQuote> {
        let id = self.try_resolve_pool(&pool_id)?;
        let p = self.pools.get(&id)?.get().into_owned();
        Some(self.quote_in(&p, &token_in, amount_in.0, builder, sender.as_ref()))
    }

    pub fn quote_swap_out(&self, pool_id: String, token_out: AccountId, amount_out: U128, builder: Option<AccountId>,
                          sender: Option<AccountId>) -> Option<SwapQuote> {
        let id = self.try_resolve_pool(&pool_id)?;
        let p = self.pools.get(&id)?.get().into_owned();
        let refuse = |why: &str| Some(SwapQuote::refused(U128(0), amount_out, why));
        if self.paused {
            return refuse("paused");
        }
        if !p.seeded {
            return refuse("unseeded");
        }
        if token_out != p.quote && token_out != p.token {
            return refuse("not_in_pool");
        }
        if amount_out.0 == 0 {
            return refuse("empty");
        }
        let token_in = if token_out == p.token { p.quote.clone() } else { p.token.clone() };
        match Self::compute_in(&p, &token_out, amount_out.0, self.rate(&p, sender.as_ref())) {
            Err(why) => refuse(why),
            Ok(amount_in) => Some(self.quote_in(&p, &token_in, amount_in, builder, sender.as_ref())),
        }
    }

    fn quote_in(&self, p: &Pool, token_in: &AccountId, amount_in: u128, builder: Option<AccountId>,
                sender: Option<&AccountId>) -> SwapQuote {
        let refuse = |why: &str| SwapQuote::refused(U128(amount_in), U128(0), why);
        if self.paused {
            return refuse("paused");
        }
        if !p.seeded {
            return refuse("unseeded");
        }
        let buying = token_in == &p.quote;
        if buying && p.real_quote.0.saturating_add(amount_in) > RESERVE_MAX {
            return refuse("reserve");
        }
        if !self.room_for(&p.platform_id) {
            return refuse("storage");
        }
        if token_in != &p.quote && token_in != &p.token {
            return refuse("not_in_pool");
        }
        let fee_bps = self.rate(p, sender);
        let (out, fee) = Self::compute(p, token_in, amount_in, fee_bps);
        if out == 0 {
            return refuse("empty");
        }
        let tier = self.builder_tier(&p.platform_id, builder.as_ref(), &p.quote, p.terms.open_builder_bps);
        let legs = Self::split(fee, &p.terms, tier);
        let y0 = p.real_quote.0 + p.virtual_quote.0;
        let x0 = p.real_token.0;
        let (x1, y1) = if buying {
            (x0 - out, y0 + amount_in - fee)
        } else {
            (x0 + amount_in, y0 - (out + fee))
        };
        let impact = impact_bps(y0, x0, y1, x1);
        SwapQuote {
            amount_in: U128(amount_in),
            amount_out: U128(out),
            fee: U128(fee),
            fee_protocol: U128(legs.0),
            fee_platform: U128(legs.3),
            fee_creator: U128(legs.2),
            fee_builder: U128(if tier == BuilderTier::Approved { legs.1 } else { 0 }),
            fee_open_builder: U128(if tier == BuilderTier::Open { legs.1 } else { 0 }),
            price_before: [U128(y0), U128(x0)],
            price_after: [U128(y1), U128(x1)],
            price_impact_bps: impact,
            fee_bps_applied: fee_bps,
            builder_tier: tier.name().map(str::to_string),
            executable: true,
            reason: None,
        }
    }

    pub fn quote_route(&self, pool_ids: Vec<String>, token_in: AccountId, amount_in: U128, sender: Option<AccountId>) -> Value {
        let refuse = |why: &str| json!({"executable": false, "reason": why, "amount_in": amount_in, "amount_out": "0", "legs": []});
        if self.paused {
            return refuse("paused");
        }
        let ids = match self.resolve_route(&pool_ids) {
            Ok(ids) => ids,
            Err(why) => return refuse(why),
        };
        match self.plan_route(&ids, &token_in, amount_in.0, sender.as_ref()) {
            Err(why) => refuse(why),
            Ok(legs) => json!({
                "executable": true, "reason": null, "amount_in": amount_in, "amount_out": U128(legs.last().unwrap().4),
                "legs": legs.iter().map(|(id, p, tin, amt, out, fee, fee_bps)| json!({
                    "pool_id": U64(*id), "token_in": tin, "amount_in": U128(*amt),
                    "token_out": if tin == &p.quote { &p.token } else { &p.quote }, "amount_out": U128(*out),
                    "fee": U128(*fee), "fee_token": p.quote, "fee_bps_applied": fee_bps})).collect::<Vec<_>>()}),
        }
    }

    pub fn quote_route_out(&self, pool_ids: Vec<String>, token_in: AccountId, amount_out: U128, sender: Option<AccountId>) -> Value {
        let refuse = |why: &str| json!({"executable": false, "reason": why, "amount_in": "0", "amount_out": amount_out, "legs": []});
        if self.paused {
            return refuse("paused");
        }
        let ids = match self.resolve_route(&pool_ids) {
            Ok(ids) => ids,
            Err(why) => return refuse(why),
        };
        if amount_out.0 == 0 {
            return refuse("empty");
        }

        let mut legs: Vec<(Pool, AccountId)> = Vec::with_capacity(ids.len());
        let mut tin = token_in.clone();
        for id in &ids {
            let p = match self.pools.get(id) {
                Some(v) => v.get().into_owned(),
                None => return refuse("no_pool"),
            };
            if !p.seeded {
                return refuse("unseeded");
            }
            if tin != p.quote && tin != p.token {
                return refuse("not_in_pool");
            }
            let tout = if tin == p.quote { p.token.clone() } else { p.quote.clone() };
            legs.push((p, tout.clone()));
            tin = tout;
        }
        let mut need = amount_out.0;
        for (p, tout) in legs.iter().rev() {
            match Self::compute_in(p, tout, need, self.rate(p, sender.as_ref())) {
                Ok(a) => need = a,
                Err(why) => return refuse(why),
            }
        }
        self.quote_route(pool_ids, token_in, U128(need), sender)
    }

    fn resolve_route(&self, pool_ids: &[String]) -> Result<Vec<u64>, &'static str> {
        if !(2..=MAX_HOPS).contains(&pool_ids.len()) {
            return Err("route_len");
        }
        let mut ids = Vec::with_capacity(pool_ids.len());
        for r in pool_ids {
            match self.try_resolve_pool(r) {
                Some(id) if !ids.contains(&id) => ids.push(id),
                Some(_) => return Err("route_repeat"),
                None => return Err("no_pool"),
            }
        }
        Ok(ids)
    }

    pub fn get_owed(&self, account: AccountId, token: AccountId) -> U128 {
        U128(self.owed.get(&(account, token)).copied().unwrap_or(0))
    }

    pub fn get_owed_total(&self, token: AccountId) -> U128 {
        U128(self.owed_total.get(&token).copied().unwrap_or(0))
    }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn apply_upgrade() {
    env::setup_panic_hook();
    require!(env::attached_deposit().is_zero(), "E_NOT_PAYABLE");
    let me: Routr = env::state_read().unwrap_or_else(|| env::panic_str("E_NO_STATE"));
    require!(env::predecessor_account_id() == me.owner, "E_OWNER_ONLY");
    require!(me.upgrade_ready_ms != 0, "E_NOT_STAGED");
    require!(env::block_timestamp_ms() >= me.upgrade_ready_ms, "E_NOT_YET");
    let code = env::input().unwrap_or_else(|| env::panic_str("E_NO_CODE"));
    require!(env::sha256_array(&code) == me.upgrade_hash, "E_CODE_HASH");

    require!(env::account_balance().as_yoctonear() >= me.headroom_for(code.len() as u64), "E_STORAGE_HEADROOM");

    let hash = Base58CryptoHash::from(me.upgrade_hash);
    emit("upgrade_dispatched", json!({"code_hash": hash}));
    let batch = env::promise_batch_create(&env::current_account_id());
    env::promise_batch_action_function_call(
        batch,
        "check_dispatch",
        json!({"code_hash": hash, "code_len": U64(code.len() as u64)}).to_string().as_bytes(),
        NearToken::from_near(0),
        GAS_CHECK_DISPATCH,
    );
    env::promise_batch_action_deploy_contract(batch, &code);
    env::promise_batch_action_function_call(batch, "migrate", b"{}", NearToken::from_near(0), GAS_MIGRATE);
    env::promise_return(batch);
}

impl Routr {

    fn require_live(&self) {
        require!(!self.paused, "E_PAUSED");
    }

    fn fee_now(p: &Pool) -> u16 {
        fee_bps_at(p.fee_bps, &p.terms.fee_schedule, p.seeded_ms.0, env::block_timestamp_ms())
    }

    fn rate(&self, p: &Pool, sender: Option<&AccountId>) -> u16 {
        sender.map_or_else(|| Self::fee_now(p), |s| self.fee_for(p, s))
    }

    fn fee_for(&self, p: &Pool, sender: &AccountId) -> u16 {
        if p.terms.fee_schedule.is_some()
            && self.platforms.get(&p.platform_id).is_some_and(|pl| {
                let pl = pl.get();
                &pl.owner == sender || pl.pool_creators.contains(sender)
            })
        {
            return p.fee_bps;
        }
        Self::fee_now(p)
    }

    fn compute(p: &Pool, token_in: &AccountId, amount_in: u128, fee_bps: u16) -> (u128, u128) {

        let y = p
            .real_quote
            .0
            .checked_add(p.virtual_quote.0)
            .unwrap_or_else(|| env::panic_str("E_OVERFLOW"));
        let x = p.real_token.0;
        if token_in == &p.quote {
            let fee = share(amount_in, fee_bps);
            let net = amount_in - fee;
            (cp_out(net, y, x), fee)
        } else if token_in == &p.token {
            let gross = cp_out(amount_in, x, y).min(p.real_quote.0);
            let fee = share(gross, fee_bps);
            (gross - fee, fee)
        } else {
            env::panic_str("E_NOT_IN_POOL")
        }
    }

    fn compute_in(p: &Pool, token_out: &AccountId, amount_out: u128, fee_bps: u16) -> Result<u128, &'static str> {
        if amount_out == 0 {
            return Err("empty");
        }
        let y = p
            .real_quote
            .0
            .checked_add(p.virtual_quote.0)
            .unwrap_or_else(|| env::panic_str("E_OVERFLOW"));
        let x = p.real_token.0;
        let (token_in, amount_in) = if token_out == &p.token {

            let net = cp_in(amount_out, y, x).ok_or("drained")?;
            (p.quote.clone(), gross_up(net, fee_bps).ok_or("drained")?)
        } else if token_out == &p.quote {

            let gross = gross_up(amount_out, fee_bps).ok_or("drained")?;
            if gross > p.real_quote.0 {
                return Err("drained");
            }
            (p.token.clone(), cp_in(gross, x, y).ok_or("drained")?)
        } else {
            return Err("not_in_pool");
        };
        if amount_in == 0 {
            return Err("empty");
        }
        let (out, _) = Self::compute(p, &token_in, amount_in, fee_bps);
        if out < amount_out {
            return Err("rounding");
        }
        Ok(amount_in)
    }

    #[allow(clippy::type_complexity)]
    fn plan_route(&self, ids: &[u64], token_in: &AccountId, amount_in: u128, sender: Option<&AccountId>)
        -> Result<Vec<(u64, Pool, AccountId, u128, u128, u128, u16)>, &'static str> {
        let mut legs: Vec<(u64, Pool, AccountId, u128, u128, u128, u16)> = Vec::with_capacity(ids.len());
        let (mut tin, mut amt) = (token_in.clone(), amount_in);

        let middle_ok = Self::balance_assets();
        for (i, id) in ids.iter().enumerate() {
            let p = match self.pools.get(id) {
                Some(v) => v.get().into_owned(),
                None => return Err("no_pool"),
            };
            if !p.seeded {
                return Err("unseeded");
            }
            if tin != p.quote && tin != p.token {
                return Err("not_in_pool");
            }
            let buying = tin == p.quote;
            if buying && p.real_quote.0.saturating_add(amt) > RESERVE_MAX {
                return Err("reserve");
            }
            let on_platform = 1 + legs.iter().filter(|l| l.1.platform_id == p.platform_id).count() as u128;
            if !self.room_for_legs(&p.platform_id, ids.len() as u128, on_platform) {
                return Err("storage");
            }

            if !buying && cp_out(amt, p.real_token.0, p.real_quote.0.saturating_add(p.virtual_quote.0)) > p.real_quote.0 {
                return Err("drained");
            }
            let fee_bps = self.rate(&p, sender);
            let (out, fee) = Self::compute(&p, &tin, amt, fee_bps);
            if out == 0 {
                return Err("empty");
            }
            let tout = if buying { p.token.clone() } else { p.quote.clone() };
            if i + 1 < ids.len() && !middle_ok.contains(&tout) {
                return Err("middle_asset");
            }
            legs.push((*id, p, tin, amt, out, fee, fee_bps));
            tin = tout;
            amt = out;
        }
        Ok(legs)
    }

    #[allow(clippy::too_many_arguments)]
    fn route(
        &mut self,
        ids: Vec<u64>,
        token_in: AccountId,
        sender: AccountId,
        recipient: AccountId,
        amount_in: u128,
        min_out: u128,
        builder: Option<AccountId>,
        delivery: Option<Delivery>,
    ) -> PromiseOrValue<U128> {

        let planned = self.plan_route(&ids, &token_in, amount_in, Some(&sender));
        let reject = |reason: &str, out: u128| {
            emit(
                "route_rejected",
                json!({"pool_ids": ids.iter().map(|i| U64(*i)).collect::<Vec<_>>(), "trader": sender,
                    "token_in": token_in, "amount_in": U128(amount_in), "out": U128(out), "min_out": U128(min_out),
                    "reason": reason}),
            );
            PromiseOrValue::Value(U128(amount_in))
        };
        let legs = match planned {
            Ok(l) => l,
            Err(why) => return reject(why, 0),
        };
        let final_out = legs.last().unwrap().4;
        if final_out < min_out {
            return reject("min_out", final_out);
        }
        if !self.may_direct(&legs.last().unwrap().1.platform_id, &sender, &recipient) {
            return reject("recipient", final_out);
        }
        let me = env::current_account_id();
        let n = legs.len();
        let mut token_out = token_in.clone();
        for (i, (id, p, tin, amt, out, fee, fee_bps)) in legs.iter().enumerate() {

            let to = if i + 1 == n { &recipient } else { &me };
            token_out = self.apply_leg(*id, p, tin, &sender, to, *amt, *out, *fee, *fee_bps, builder.clone(), Some([i as u8, n as u8]));
        }
        let last_platform = legs.last().unwrap().1.platform_id.clone();
        emit(
            "route",
            json!({"pool_ids": ids.iter().map(|i| U64(*i)).collect::<Vec<_>>(), "trader": sender, "recipient": recipient,
                "token_in": token_in, "amount_in": U128(amount_in), "token_out": token_out, "amount_out": U128(final_out),
                "delivery": if delivery.is_some() {"call"} else {"transfer"}}),
        );
        if self.add_pending(&(recipient.clone(), token_out.clone()), final_out) {
            self.charge_key(&last_platform);
            self.charge_key(&last_platform);
            Self::pending_payer_set(&(recipient.clone(), token_out.clone()), &last_platform);
        }
        if self.add_owed_total(&token_out, final_out) {
            self.charge_key(&last_platform);
        }
        self.deliver(&token_out, &recipient, final_out, "swap", &last_platform, delivery.as_ref().map(|d| (d, &sender)))
            .detach();
        PromiseOrValue::Value(U128(0))
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_leg(
        &mut self,
        pool_id: u64,
        p: &Pool,
        token_in: &AccountId,
        sender: &AccountId,
        recipient: &AccountId,
        amount_in: u128,
        out: u128,
        fee: u128,
        fee_bps: u16,
        builder: Option<AccountId>,
        hop: Option<[u8; 2]>,
    ) -> AccountId {
        let buying = token_in == &p.quote;

        let (token_out, quote_moved, gross) = if buying {
            (p.token.clone(), amount_in - fee, amount_in)
        } else {
            (p.quote.clone(), out + fee, out + fee)
        };

        let tier = self.builder_tier(&p.platform_id, builder.as_ref(), &p.quote, p.terms.open_builder_bps);
        let builder = if tier == BuilderTier::None { None } else { builder };
        let (to_protocol, to_builder, to_creator, to_platform) = Self::split(fee, &p.terms, tier);
        let (to_approved, to_open) = match tier {
            BuilderTier::Approved => (to_builder, 0),
            BuilderTier::Open => (0, to_builder),
            BuilderTier::None => (0, 0),
        };
        {
            let pm = self.pools.get_mut(&pool_id).unwrap().get_mut();
            if buying {
                pm.real_quote = U128(pm.real_quote.0 + quote_moved);
                pm.real_token = U128(pm.real_token.0 - out);
            } else {
                pm.real_token = U128(pm.real_token.0 + amount_in);
                pm.real_quote = U128(pm.real_quote.0 - quote_moved);
            }
            pm.volume_quote = U128(pm.volume_quote.0.saturating_add(gross));
            pm.fees_quote = U128(pm.fees_quote.0.saturating_add(fee));
            pm.fees_protocol = U128(pm.fees_protocol.0.saturating_add(to_protocol));
            pm.fees_platform = U128(pm.fees_platform.0.saturating_add(to_platform));
            pm.fees_creator = U128(pm.fees_creator.0.saturating_add(to_creator));
            pm.fees_builder = U128(pm.fees_builder.0.saturating_add(to_approved));
            pm.fees_open_builder = U128(pm.fees_open_builder.0.saturating_add(to_open));
            pm.swaps = pm.swaps.saturating_add(1);
        }
        let quote = p.quote.clone();
        let pid = p.platform_id.clone();

        self.book(&p.terms.protocol_recipient.clone(), &quote, to_protocol, &pid, &pid);
        self.book(&p.terms.platform_recipient.clone(), &quote, to_platform, &pid, &pid);
        self.book(&p.creator, &quote, to_creator, &pid, &pid);
        if let Some(b) = &builder {
            let payer = if tier == BuilderTier::Open { format!("@{b}") } else { pid.clone() };
            self.book(b, &quote, to_builder, &payer, &pid);
        }
        let after = self.pools.get(&pool_id).unwrap().get().into_owned();
        emit(
            "swap",
            json!({"pool_id": U64(pool_id), "token": p.token, "quote": p.quote, "trader": sender, "recipient": recipient,
                "side": if buying {"buy"} else {"sell"},
                "token_in": token_in, "amount_in": U128(amount_in), "token_out": token_out, "amount_out": U128(out),
                "fee": U128(fee), "fee_bps_applied": fee_bps, "fee_protocol": U128(to_protocol), "fee_platform": U128(to_platform),
                "fee_creator": U128(to_creator), "fee_builder": U128(to_approved), "fee_open_builder": U128(to_open),
                "builder": builder, "builder_tier": tier.name(),
                "platform_id": p.platform_id, "hop": hop, "real_token": after.real_token, "real_quote": after.real_quote}),
        );
        token_out
    }

    fn split(fee: u128, t: &PoolTerms, tier: BuilderTier) -> (u128, u128, u128, u128) {
        let to_protocol = share(fee, t.protocol_fee_bps);
        let rest = fee - to_protocol;
        let to_builder = match tier {
            BuilderTier::Approved => share(rest, t.builder_bps),
            BuilderTier::Open => share(rest, t.open_builder_bps),
            BuilderTier::None => 0,
        };
        let to_creator = share(rest, t.creator_bps);
        (to_protocol, to_builder, to_creator, rest - to_builder - to_creator)
    }

    fn builder_tier(&self, platform_id: &str, builder: Option<&AccountId>, quote: &AccountId, open_bps: u16) -> BuilderTier {
        let Some(b) = builder else { return BuilderTier::None };
        if self.builder_approved(platform_id, b) {
            return BuilderTier::Approved;
        }
        if open_bps == 0 {
            return BuilderTier::None;
        }
        let Some(credit) = Self::builder_credit(b.as_str()) else { return BuilderTier::None };
        let key = env::storage_byte_cost().as_yoctonear() * BUILDER_KEY_BYTES as u128;
        if self.owed.contains_key(&(b.clone(), quote.clone())) || credit >= key * BUILDER_KEYS {
            BuilderTier::Open
        } else {
            BuilderTier::None
        }
    }

    fn delivery(msg: Option<String>, gas: Option<U64>) -> Option<Delivery> {
        let msg = msg?;
        require!(msg.len() <= DELIVER_MSG_MAX, "E_DELIVER_MSG");
        let gas = gas.map_or(GAS_DELIVER_CALL_DEFAULT, |g| Gas::from_gas(g.0));
        require!(gas.as_gas() >= GAS_DELIVER_CALL_MIN.as_gas() && gas.as_gas() <= GAS_DELIVER_CALL_MAX.as_gas(), "E_DELIVER_GAS");
        Some(Delivery { msg, gas })
    }

    fn may_direct(&self, platform_id: &str, sender: &AccountId, recipient: &AccountId) -> bool {
        recipient == sender
            || self
                .platforms
                .get(platform_id)
                .is_some_and(|p| p.get().pool_creators.contains(sender))
    }

    fn builder_approved(&self, platform_id: &str, builder: &AccountId) -> bool {
        self.platforms
            .get(platform_id)
            .is_some_and(|p| p.get().builders.contains(builder))
    }

    fn swap(
        &mut self,
        pool_id: u64,
        token_in: AccountId,
        sender: AccountId,
        recipient: AccountId,
        amount_in: u128,
        min_out: u128,
        builder: Option<AccountId>,
        delivery: Option<Delivery>,
    ) -> PromiseOrValue<U128> {
        let p = self
            .pools
            .get(&pool_id)
            .map(|v| v.get().into_owned())
            .unwrap_or_else(|| env::panic_str("E_NO_POOL"));
        require!(p.seeded, "E_NOT_SEEDED");
        let fee_bps = self.fee_for(&p, &sender);
        let (out, fee) = Self::compute(&p, &token_in, amount_in, fee_bps);

        let room = self.room_for(&p.platform_id);
        let buying = token_in == p.quote;

        let fits = !buying || p.real_quote.0.saturating_add(amount_in) <= RESERVE_MAX;
        let own = self.may_direct(&p.platform_id, &sender, &recipient);
        if out == 0 || out < min_out || !room || !fits || !own {
            emit(
                "swap_rejected",
                json!({"pool_id": U64(pool_id), "trader": sender, "token_in": token_in, "amount_in": U128(amount_in),
                    "out": U128(out), "min_out": U128(min_out),
                    "reason": if !own {"recipient"} else if !room {"storage"} else if !fits {"reserve"} else {"min_out"}}),
            );
            return PromiseOrValue::Value(U128(amount_in));
        }
        let token_out = self.apply_leg(pool_id, &p, &token_in, &sender, &recipient, amount_in, out, fee, fee_bps, builder, None);

        if self.add_pending(&(recipient.clone(), token_out.clone()), out) {

            self.charge_key(&p.platform_id);
            self.charge_key(&p.platform_id);
            Self::pending_payer_set(&(recipient.clone(), token_out.clone()), &p.platform_id);
        }
        if self.add_owed_total(&token_out, out) {
            self.charge_key(&p.platform_id);
        }
        self.deliver(&token_out, &recipient, out, "swap", &p.platform_id, delivery.as_ref().map(|d| (d, &sender)))
            .detach();
        PromiseOrValue::Value(U128(0))
    }

    fn deliver(
        &mut self,
        token: &AccountId,
        to: &AccountId,
        amount: u128,
        why: &str,
        platform_id: &str,
        call: Option<(&Delivery, &AccountId)>,
    ) -> Promise {
        self.inflight += 1;
        let p = Promise::new(token.clone());
        let p = match call {
            Some((d, _)) => p.function_call(
                "ft_transfer_call",
                json!({"receiver_id": to, "amount": U128(amount), "msg": d.msg}).to_string().into_bytes(),
                ONE_YOCTO,
                d.gas,
            ),
            None => p.function_call(
                "ft_transfer",
                json!({"receiver_id": to, "amount": U128(amount)}).to_string().into_bytes(),
                ONE_YOCTO,
                GAS_FT_TRANSFER,
            ),
        };
        p.then(
            Self::ext(env::current_account_id())
                .with_static_gas(GAS_ON_DELIVERED)
                .with_unused_gas_weight(0)
                .on_delivered(
                    token.clone(),
                    to.clone(),
                    U128(amount),
                    why.to_string(),
                    platform_id.to_string(),
                    call.map(|(_, s)| s.clone()),
                ),
        )
    }

    fn book(&mut self, to: &AccountId, token: &AccountId, amount: u128, payer: &str, platform_id: &str) {
        if amount == 0 {
            return;
        }
        let key = (to.clone(), token.clone());
        let existing = self.owed.get(&key).copied();
        self.owed.insert(key.clone(), existing.unwrap_or(0) + amount);
        if existing.is_none() {
            let payer = payer.strip_prefix('@').map_or(payer.to_string(), |b| format!("#{b}"));
            let payer = payer.as_str();

            Self::owed_payer_set(&key, payer);
            self.charge_key(payer);
            self.charge_key(payer);
        }
        if self.add_owed_total(token, amount) {
            self.charge_key(platform_id);
        }
    }

    fn add_owed_total(&mut self, token: &AccountId, amount: u128) -> bool {
        let t = self.owed_total.get(token).copied();
        self.owed_total.insert(token.clone(), t.unwrap_or(0) + amount);
        t.is_none()
    }

    fn try_resolve_pool(&self, r: &str) -> Option<u64> {
        if let Ok(id) = r.parse::<u64>() {
            return Some(id);
        }
        self.pool_ids.get(r).copied()
    }

    fn resolve_pool(&self, r: &str) -> u64 {
        self.try_resolve_pool(r)
            .unwrap_or_else(|| env::panic_str("E_NO_POOL"))
    }

    fn room_for(&self, platform_id: &str) -> bool {
        self.room_for_legs(platform_id, 1, 1)
    }

    fn room_for_legs(&self, platform_id: &str, legs: u128, on_platform: u128) -> bool {
        let byte = env::storage_byte_cost().as_yoctonear();
        let per_swap = byte * SWAP_STORAGE_BYTES as u128;
        let stake = byte * env::storage_usage() as u128;

        let reserved = byte * 2 * BUILDER_KEY_BYTES as u128 * (self.inflight as u128 + 1);
        let credit = self
            .platforms
            .get(platform_id)
            .map(|x| x.get().storage_credit.0)
            .unwrap_or(0);
        env::account_balance().as_yoctonear().saturating_sub(stake) >= per_swap * 2 * legs + reserved
            && credit >= per_swap * 2 * on_platform
    }

    fn charge_key(&mut self, payer: &str) {
        self.credit_move(payer, false);
    }

    fn refund_key(&mut self, payer: &str) {
        self.credit_move(payer, true);
    }

    fn credit_move(&mut self, payer: &str, refund: bool) {
        if payer.is_empty() {
            return;
        }
        let bytes = if payer.starts_with('#') { BUILDER_KEY_BYTES } else { KEY_BYTES };
        let cost = env::storage_byte_cost().as_yoctonear() * bytes as u128;
        if let Some(b) = payer.strip_prefix('#').or_else(|| payer.strip_prefix('@')) {
            if let Some(c) = Self::builder_credit(b) {
                Self::builder_credit_write(b, if refund { c.saturating_add(cost) } else { c.saturating_sub(cost) });
            }
            return;
        }
        if let Some(p) = self.platforms.get_mut(payer) {
            let p = p.get_mut();
            p.storage_credit = U128(if refund { p.storage_credit.0.saturating_add(cost) } else { p.storage_credit.0.saturating_sub(cost) });
        }
    }

    fn builder_credit_key(builder: &str) -> Vec<u8> {
        let mut k = b"BCRD".to_vec();
        k.extend_from_slice(builder.as_bytes());
        k
    }

    fn builder_credit(builder: &str) -> Option<u128> {
        let v = env::storage_read(&Self::builder_credit_key(builder))?;
        let bytes: [u8; 16] = v.try_into().ok()?;
        Some(u128::from_le_bytes(bytes))
    }

    fn builder_credit_write(builder: &str, credit: u128) {
        env::storage_write(&Self::builder_credit_key(builder), &credit.to_le_bytes());
    }

    fn pending_payer_key(key: &(AccountId, AccountId)) -> Vec<u8> {
        let mut k = b"PPAY".to_vec();
        k.extend_from_slice(key.0.as_bytes());
        k.push(b'|');
        k.extend_from_slice(key.1.as_bytes());
        k
    }

    fn pending_payer_set(key: &(AccountId, AccountId), platform_id: &str) {
        env::storage_write(&Self::pending_payer_key(key), platform_id.as_bytes());
    }

    fn pending_payer_take(key: &(AccountId, AccountId)) -> Option<String> {
        let k = Self::pending_payer_key(key);
        let v = env::storage_read(&k)?;
        env::storage_remove(&k);
        String::from_utf8(v).ok()
    }

    fn owed_payer_key(key: &(AccountId, AccountId)) -> Vec<u8> {
        let mut k = b"OPAY".to_vec();
        k.extend_from_slice(key.0.as_bytes());
        k.push(b'|');
        k.extend_from_slice(key.1.as_bytes());
        k
    }

    fn owed_payer_set(key: &(AccountId, AccountId), platform_id: &str) {
        if platform_id.is_empty() {
            return;
        }
        env::storage_write(&Self::owed_payer_key(key), platform_id.as_bytes());
    }

    fn owed_payer_take(key: &(AccountId, AccountId)) -> Option<String> {
        let k = Self::owed_payer_key(key);
        let v = env::storage_read(&k)?;
        env::storage_remove(&k);
        String::from_utf8(v).ok()
    }

    fn add_pending(&mut self, key: &(AccountId, AccountId), amount: u128) -> bool {
        let p = self.pending.get(key).copied();
        self.pending.insert(key.clone(), p.unwrap_or(0) + amount);
        p.is_none()
    }

    fn settle_storage(before: u64, keep: u128) {
        let after = env::storage_usage();
        let cost = env::storage_byte_cost().as_yoctonear() * after.saturating_sub(before) as u128 + keep;
        let paid = env::attached_deposit().as_yoctonear();
        require!(paid >= cost, "E_STORAGE_DEPOSIT");
        if paid > cost {
            Promise::new(env::predecessor_account_id())
                .transfer(NearToken::from_yoctonear(paid - cost))
                .detach();
        }
    }
}

