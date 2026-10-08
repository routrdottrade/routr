use super::*;
use near_sdk::PublicKey;

pub const BALANCE_REGISTER_MIN: NearToken = NearToken::from_millinear(10);

pub const MAX_BALANCE_ASSETS: usize = 16;

pub const MAX_SESSIONS: usize = 8;
pub const MAX_SESSION_PLATFORMS: usize = 8;

pub const MAX_OPEN_ORDERS: usize = 32;
pub const DAY_MS: u64 = 86_400_000;

pub const ORDER_MAX_MS: u64 = 90 * DAY_MS;

pub const SESSION_MAX_MS: u64 = 365 * DAY_MS;

pub const SESSION_TIP_CAP_BPS: u16 = 100;
const GAS_ON_BALANCE_WITHDRAWN: Gas = Gas::from_tgas(10);

const TIP_RECORD_BYTES: u64 = 400;

pub(crate) const BALANCE_ASSETS_KEY: &[u8] = b"BAST";
const NEXT_ORDER_KEY: &[u8] = b"ONXT";

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub public_key: PublicKey,
    pub asset: AccountId,

    pub max_per_swap: U128,

    pub max_per_day: U128,

    pub expires_ms: U64,

    pub platforms: Vec<String>,

    pub day: U64,
    pub spent: U128,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, Default)]
pub struct BalanceAccount {

    pub storage_credit: u128,
    pub sessions: Vec<Session>,

    pub orders: Vec<u64>,
}

#[near(serializers = [borsh])]
pub enum VAccount {
    V1(BalanceAccount),
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {

    Buy,

    Sell,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub id: U64,
    pub owner: AccountId,
    pub pool_id: U64,
    pub side: OrderSide,
    pub token_in: AccountId,
    pub token_out: AccountId,
    pub amount_in: U128,

    pub min_out: U128,

    pub tip: U128,
    pub expires_ms: U64,
    pub created_ms: U64,
    pub builder: Option<AccountId>,

    pub storage_charged: U128,
}

#[near(serializers = [borsh])]
pub enum VOrder {
    V1(Order),
}

pub fn utc_day(now_ms: u64) -> u64 {
    now_ms / DAY_MS
}

pub fn session_spend(s: &Session, now_ms: u64, platform_id: &str, amount: u128) -> Result<(u64, u128), &'static str> {
    if now_ms >= s.expires_ms.0 {
        return Err("E_SESSION_EXPIRED");
    }
    if !s.platforms.is_empty() && !s.platforms.iter().any(|p| p == platform_id) {
        return Err("E_SESSION_PLATFORM");
    }
    if amount > s.max_per_swap.0 {
        return Err("E_SESSION_PER_SWAP");
    }
    let day = utc_day(now_ms);
    let spent = if s.day.0 == day { s.spent.0 } else { 0 };
    match spent.checked_add(amount) {
        Some(t) if t <= s.max_per_day.0 => Ok((day, t)),
        _ => Err("E_SESSION_PER_DAY"),
    }
}

pub fn check_order_terms(now_ms: u64, amount_in: u128, min_out: u128, expires_ms: u64) -> Result<(), &'static str> {
    if amount_in == 0 {
        return Err("E_AMOUNT");
    }
    if min_out == 0 {
        return Err("E_MIN_OUT");
    }
    if expires_ms <= now_ms || expires_ms - now_ms > ORDER_MAX_MS {
        return Err("E_EXPIRY");
    }
    Ok(())
}

pub fn order_take(side: OrderSide, out: u128, tip: u128, min_out: u128) -> Result<u128, &'static str> {
    let take = match side {
        OrderSide::Buy => out,
        OrderSide::Sell => out.checked_sub(tip).ok_or("limit")?,
    };
    if out == 0 || take < min_out {
        return Err("limit");
    }
    Ok(take)
}

fn key2(prefix: &[u8], a: &str, b: &str) -> Vec<u8> {
    let mut k = prefix.to_vec();
    k.extend_from_slice(a.as_bytes());
    k.push(b'|');
    k.extend_from_slice(b.as_bytes());
    k
}

fn key1(prefix: &[u8], a: &[u8]) -> Vec<u8> {
    let mut k = prefix.to_vec();
    k.extend_from_slice(a);
    k
}

fn read_u128(k: &[u8]) -> Option<u128> {
    let v = env::storage_read(k)?;
    Some(u128::from_le_bytes(v.try_into().ok()?))
}

#[near]
impl Routr {

    #[payable]
    pub fn set_balance_assets(&mut self, assets: Vec<AccountId>) {
        require!(env::predecessor_account_id() == self.owner, "E_OWNER_ONLY");
        require!(assets.len() <= MAX_BALANCE_ASSETS, "E_TOO_MANY_ASSETS");
        for (i, a) in assets.iter().enumerate() {
            require!(!assets[..i].contains(a) && *a != env::current_account_id(), "E_ASSETS");
        }
        let before = env::storage_usage();
        env::storage_write(BALANCE_ASSETS_KEY, &near_sdk::borsh::to_vec(&assets).unwrap());
        Self::settle_storage(before, 0);
        emit("balance_assets_changed", json!({"assets": assets}));
    }

    #[payable]
    pub fn balance_register(&mut self) {
        self.require_live();
        let who = env::predecessor_account_id();
        let paid = env::attached_deposit().as_yoctonear();
        let a = match Self::account_get(&who) {
            Some(mut a) => {
                require!(paid > 0, "E_DEPOSIT");
                a.storage_credit = a.storage_credit.saturating_add(paid);
                Self::account_put(&who, &a);
                a
            }
            None => {
                require!(paid >= BALANCE_REGISTER_MIN.as_yoctonear(), "E_STORAGE_DEPOSIT");
                let start = env::storage_usage();
                let mut a = BalanceAccount { storage_credit: paid, ..Default::default() };
                Self::account_commit(&who, &mut a, start, 0, true);
                a
            }
        };
        emit("balance_registered", json!({"account": who, "amount": U128(paid), "credit": U128(a.storage_credit)}));
    }

    #[payable]
    pub fn balance_credit_withdraw(&mut self, amount: Option<U128>) -> U128 {
        require!(env::attached_deposit() == ONE_YOCTO, "E_ONE_YOCTO");
        let who = env::predecessor_account_id();
        let mut a = Self::account_get(&who).unwrap_or_else(|| env::panic_str("E_NOT_REGISTERED"));
        let amt = amount.map_or(a.storage_credit, |x| x.0);
        require!(amt > 0 && amt <= a.storage_credit, "E_CREDIT");
        a.storage_credit -= amt;
        Self::account_put(&who, &a);

        Promise::new(who.clone()).transfer(NearToken::from_yoctonear(amt)).then(
            Self::ext(env::current_account_id())
                .with_static_gas(GAS_ON_BALANCE_WITHDRAWN)
                .with_unused_gas_weight(0)
                .on_credit_withdrawn(who, U128(amt)),
        ).detach();
        U128(amt)
    }

    #[private]
    #[allow(deprecated)]
    pub fn on_credit_withdrawn(&mut self, account: AccountId, amount: U128) -> bool {
        let ok = matches!(env::promise_result(0), PromiseResult::Successful(_));
        let mut credit = 0;
        if let Some(mut a) = Self::account_get(&account) {
            if !ok {
                a.storage_credit = a.storage_credit.saturating_add(amount.0);
                Self::account_put(&account, &a);
            }
            credit = a.storage_credit;
        }
        emit("balance_credit_withdrawn", json!({"account": account, "amount": amount, "ok": ok, "credit": U128(credit)}));
        ok
    }

    #[payable]
    pub fn withdraw_balance(&mut self, asset: AccountId, amount: Option<U128>) -> Promise {
        require!(env::attached_deposit() == ONE_YOCTO, "E_ONE_YOCTO");
        let who = env::predecessor_account_id();
        let bal = Self::balance_get(&who, &asset).unwrap_or(0);
        let amt = amount.map_or(bal, |x| x.0);
        require!(amt > 0 && amt <= bal, "E_BALANCE");

        Self::balance_put(&who, &asset, bal - amt);
        Self::held_put(&asset, Self::held_get(&asset).saturating_sub(amt));
        Promise::new(asset.clone())
            .function_call(
                "ft_transfer".to_string(),
                json!({"receiver_id": who, "amount": U128(amt)}).to_string().into_bytes(),
                ONE_YOCTO,
                GAS_FT_TRANSFER,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_BALANCE_WITHDRAWN)
                    .with_unused_gas_weight(0)
                    .on_balance_withdrawn(who, asset, U128(amt)),
            )
    }

    #[private]
    #[allow(deprecated)]
    pub fn on_balance_withdrawn(&mut self, account: AccountId, asset: AccountId, amount: U128) -> bool {
        let ok = matches!(env::promise_result(0), PromiseResult::Successful(_));
        let bal = Self::balance_get(&account, &asset);
        let mut a = Self::account_get(&account);
        let start = env::storage_usage();

        let left = if ok {
            bal.unwrap_or(0)
        } else {
            let b = bal.unwrap_or(0).saturating_add(amount.0);
            Self::balance_put(&account, &asset, b);
            Self::held_put(&asset, Self::held_get(&asset).saturating_add(amount.0));
            b
        };
        if let Some(a) = a.as_mut() {
            Self::account_commit(&account, a, start, 0, false);
        }
        emit("balance_withdraw", json!({"account": account, "asset": asset, "amount": amount, "ok": ok, "balance": U128(left)}));
        ok
    }

    #[payable]
    pub fn add_session(
        &mut self,
        public_key: PublicKey,
        asset: AccountId,
        max_per_swap: U128,
        max_per_day: U128,
        expires_ms: U64,
        platforms: Vec<String>,
    ) {
        self.require_live();
        require!(env::attached_deposit() == ONE_YOCTO, "E_ONE_YOCTO");
        let who = env::predecessor_account_id();
        let mut a = Self::account_get(&who).unwrap_or_else(|| env::panic_str("E_NOT_REGISTERED"));
        require!(Self::balance_assets().contains(&asset), "E_NOT_BALANCE_ASSET");
        require!(max_per_swap.0 > 0 && max_per_swap.0 <= max_per_day.0, "E_SESSION_LIMITS");
        let now = env::block_timestamp_ms();
        require!(expires_ms.0 > now && expires_ms.0 - now <= SESSION_MAX_MS, "E_EXPIRY");
        require!(
            platforms.len() <= MAX_SESSION_PLATFORMS && platforms.iter().all(|p| valid_platform_id(p)),
            "E_SESSION_PLATFORMS"
        );
        let start = env::storage_usage();
        let mut s = Session {
            public_key: public_key.clone(),
            asset,
            max_per_swap,
            max_per_day,
            expires_ms,
            platforms,
            day: U64(0),
            spent: U128(0),
        };
        match a.sessions.iter_mut().find(|x| x.public_key == s.public_key && x.asset == s.asset) {
            Some(x) => {
                (s.day, s.spent) = (x.day, x.spent);
                *x = s.clone();
            }
            None => {
                require!(a.sessions.len() < MAX_SESSIONS, "E_TOO_MANY_SESSIONS");
                a.sessions.push(s.clone());
            }
        }
        Self::account_commit(&who, &mut a, start, 0, true);
        emit(
            "session_added",
            json!({"account": who, "public_key": public_key, "asset": s.asset, "max_per_swap": s.max_per_swap,
                "max_per_day": s.max_per_day, "expires_ms": s.expires_ms, "platforms": s.platforms}),
        );
    }

    #[payable]
    pub fn remove_session(&mut self, public_key: PublicKey) {
        let who = env::predecessor_account_id();
        let by_itself = env::attached_deposit().is_zero()
            && env::signer_account_id() == who
            && env::signer_account_pk() == public_key;
        require!(by_itself || env::attached_deposit() == ONE_YOCTO, "E_ONE_YOCTO");
        let mut a = Self::account_get(&who).unwrap_or_else(|| env::panic_str("E_NOT_REGISTERED"));
        let n = a.sessions.len();
        a.sessions.retain(|s| s.public_key != public_key);
        require!(a.sessions.len() < n, "E_NO_SESSION");
        let start = env::storage_usage();
        Self::account_commit(&who, &mut a, start, 0, false);
        emit("session_removed", json!({"account": who, "public_key": public_key, "by": if by_itself {"session"} else {"owner"}}));
    }

    #[payable]
    pub fn swap_from_balance(&mut self, pool_id: String, amount_in: U128, min_out: U128, builder: Option<AccountId>) -> U128 {
        self.require_live();
        let who = env::predecessor_account_id();
        let id = self.resolve_pool(&pool_id);
        let p = self.seeded_pool(id);
        if let Some(b) = &builder {
            require!(*b != env::current_account_id(), "E_BUILDER");
        }
        require!(amount_in.0 > 0, "E_AMOUNT");
        let asset = p.quote.clone();
        let mut a = Self::account_get(&who).unwrap_or_else(|| env::panic_str("E_NOT_REGISTERED"));
        let by_session = Self::authorize(&mut a, Some((&asset, amount_in.0, &p.platform_id)));
        let bal = Self::balance_get(&who, &asset).unwrap_or(0);
        require!(bal >= amount_in.0, "E_BALANCE");
        let fee_bps = self.fee_for(&p, &who);
        let (out, fee) = Self::compute(&p, &asset, amount_in.0, fee_bps);
        self.require_tradeable(&p, OrderSide::Buy, amount_in.0);
        require!(out > 0 && out >= min_out.0, "E_MIN_OUT");
        Self::balance_put(&who, &asset, bal - amount_in.0);
        Self::held_put(&asset, Self::held_get(&asset).saturating_sub(amount_in.0));
        Self::account_put(&who, &a);
        let token_out = self.apply_leg(id, &p, &asset, &who, &who, amount_in.0, out, fee, fee_bps, builder, None);
        self.payout(&token_out, &who, out, "swap", &p.platform_id);
        emit(
            "balance_swap",
            json!({"account": who, "pool_id": U64(id), "asset": asset, "amount_in": U128(amount_in.0),
                "token_out": token_out, "amount_out": U128(out), "by_session": by_session,
                "balance": U128(bal - amount_in.0)}),
        );
        U128(out)
    }

    #[payable]
    pub fn place_order(
        &mut self,
        pool_id: String,
        amount_in: U128,
        min_out: U128,
        expires_ms: U64,
        tip: U128,
        builder: Option<AccountId>,
    ) -> U64 {
        self.require_live();
        let who = env::predecessor_account_id();
        let id = self.resolve_pool(&pool_id);
        let p = self.seeded_pool(id);
        if let Some(b) = &builder {
            require!(*b != env::current_account_id(), "E_BUILDER");
        }
        check_order_terms(env::block_timestamp_ms(), amount_in.0, min_out.0, expires_ms.0).unwrap_or_else(|e| env::panic_str(e));
        let escrow = amount_in.0.checked_add(tip.0).unwrap_or_else(|| env::panic_str("E_AMOUNT"));
        let asset = p.quote.clone();
        let mut a = Self::account_get(&who).unwrap_or_else(|| env::panic_str("E_NOT_REGISTERED"));
        if Self::authorize(&mut a, Some((&asset, escrow, &p.platform_id))) {
            require!(tip.0 <= share(amount_in.0, SESSION_TIP_CAP_BPS), "E_SESSION_TIP");
        }
        let bal = Self::balance_get(&who, &asset).unwrap_or(0);
        require!(bal >= escrow, "E_BALANCE");
        let start = env::storage_usage();

        Self::balance_put(&who, &asset, bal - escrow);
        U64(Self::open_order(&who, &mut a, start, OrderSide::Buy, &p, amount_in.0, min_out.0, expires_ms.0, tip.0, builder))
    }

    pub fn fill_order(&mut self, order_id: U64) -> U128 {
        self.require_live();
        let o = Self::order_get(order_id.0).unwrap_or_else(|| env::panic_str("E_NO_ORDER"));
        require!(env::block_timestamp_ms() < o.expires_ms.0, "E_ORDER_EXPIRED");
        let p = self.seeded_pool(o.pool_id.0);
        let keeper = env::predecessor_account_id();
        let fee_bps = self.fee_for(&p, &o.owner);
        let (out, fee) = Self::compute(&p, &o.token_in, o.amount_in.0, fee_bps);
        let take = order_take(o.side, out, o.tip.0, o.min_out.0).unwrap_or_else(|_| env::panic_str("E_BELOW_LIMIT"));
        self.require_tradeable(&p, o.side, o.amount_in.0);

        self.close_order(&o, false);
        let token_out = self.apply_leg(o.pool_id.0, &p, &o.token_in, &o.owner, &o.owner, o.amount_in.0, out, fee, fee_bps, o.builder.clone(), None);
        self.payout(&token_out, &o.owner, take, "order", &p.platform_id);

        Self::credit_tip(&keeper, &p.quote, o.tip.0);
        emit(
            "order_filled",
            json!({"order_id": o.id, "owner": o.owner, "keeper": keeper, "pool_id": o.pool_id, "side": o.side,
                "token_in": o.token_in, "amount_in": o.amount_in, "token_out": token_out, "amount_out": U128(take),
                "min_out": o.min_out, "tip": o.tip, "fee": U128(fee), "fee_bps_applied": fee_bps}),
        );
        U128(take)
    }

    #[payable]
    pub fn cancel_order(&mut self, order_id: U64) {
        let o = Self::order_get(order_id.0).unwrap_or_else(|| env::panic_str("E_NO_ORDER"));
        let who = env::predecessor_account_id();
        require!(who == o.owner, "E_NOT_OWNER");
        let mut a = Self::account_get(&who).unwrap_or_else(|| env::panic_str("E_NOT_REGISTERED"));
        let by_session = Self::authorize(&mut a, None);
        self.close_order(&o, true);
        emit("order_cancelled", json!({"order_id": o.id, "owner": o.owner, "side": o.side, "token": o.token_in,
            "refund": U128(Self::escrow_of(&o)), "by_session": by_session}));
    }

    pub fn expire_order(&mut self, order_id: U64) {
        let o = Self::order_get(order_id.0).unwrap_or_else(|| env::panic_str("E_NO_ORDER"));
        require!(env::block_timestamp_ms() >= o.expires_ms.0, "E_NOT_EXPIRED");
        self.close_order(&o, true);
        emit("order_expired", json!({"order_id": o.id, "owner": o.owner, "side": o.side, "token": o.token_in,
            "refund": U128(Self::escrow_of(&o)), "by": env::predecessor_account_id()}));
    }

    pub fn get_balance_assets(&self) -> Vec<AccountId> {
        Self::balance_assets()
    }

    pub fn get_balance(&self, account: AccountId, asset: AccountId) -> U128 {
        U128(Self::balance_get(&account, &asset).unwrap_or(0))
    }

    pub fn get_balance_account(&self, account: AccountId) -> Option<Value> {
        let a = Self::account_get(&account)?;
        Some(json!({"account": account, "storage_credit": U128(a.storage_credit), "open_orders": a.orders.len(),
            "sessions": a.sessions.len(), "max_open_orders": MAX_OPEN_ORDERS, "max_sessions": MAX_SESSIONS}))
    }

    pub fn get_sessions(&self, account: AccountId) -> Vec<Session> {
        let today = utc_day(env::block_timestamp_ms());
        Self::account_get(&account).map_or(vec![], |a| {
            a.sessions
                .into_iter()
                .map(|mut s| {
                    if s.day.0 != today {
                        (s.day, s.spent) = (U64(today), U128(0));
                    }
                    s
                })
                .collect()
        })
    }

    pub fn get_order(&self, order_id: U64) -> Option<Order> {
        Self::order_get(order_id.0)
    }

    pub fn get_orders(&self, account: AccountId) -> Vec<Order> {
        Self::account_get(&account).map_or(vec![], |a| a.orders.iter().filter_map(|i| Self::order_get(*i)).collect())
    }

    pub fn get_open_orders(&self, from: U64, limit: u64) -> Vec<Order> {
        let end = from.0.saturating_add(limit.min(100)).min(Self::next_order());
        (from.0..end).filter_map(Self::order_get).collect()
    }

    pub fn get_order_count(&self) -> U64 {
        U64(Self::next_order())
    }

    pub fn get_held_total(&self, token: AccountId) -> U128 {
        U128(Self::held_get(&token))
    }

    pub fn quote_fill(&self, order_id: U64, filler: Option<AccountId>) -> Value {
        let Some(o) = Self::order_get(order_id.0) else {
            return json!({"executable": false, "reason": "no_order"});
        };
        let fee_bps = self.pools.get(&o.pool_id.0).map(|v| self.fee_for(&v.get(), &o.owner)).unwrap_or(0);
        let (out, fee) = self.pools.get(&o.pool_id.0).map_or((0, 0), |v| Self::compute(&v.get(), &o.token_in, o.amount_in.0, fee_bps));
        let p = self.pools.get(&o.pool_id.0).map(|v| v.get().into_owned());
        let take = order_take(o.side, out, o.tip.0, o.min_out.0);
        let reason = if self.paused {
            Some("paused")
        } else if env::block_timestamp_ms() >= o.expires_ms.0 {
            Some("expired")
        } else if take.is_err() {
            Some("limit")
        } else if !p.as_ref().is_some_and(|p| self.room_for(&p.platform_id)) {
            Some("storage")
        } else if o.side == OrderSide::Buy && p.as_ref().is_some_and(|p| p.real_quote.0.saturating_add(o.amount_in.0) > RESERVE_MAX) {
            Some("reserve")
        } else {

            filler.as_ref().filter(|_| o.tip.0 > 0).and_then(|f| match (Self::account_get(f), p.as_ref()) {
                (None, _) => Some("filler_not_registered"),
                (Some(a), Some(p)) if Self::balance_get(f, &p.quote).is_none()
                    && a.storage_credit < env::storage_byte_cost().as_yoctonear() * TIP_RECORD_BYTES as u128 => Some("filler_storage"),
                _ => None,
            })
        };
        json!({"executable": reason.is_none(), "reason": reason, "order_id": o.id, "amount_out": U128(out),
            "owner_gets": U128(take.unwrap_or(0)), "min_out": o.min_out, "tip": o.tip, "fee": U128(fee),
            "fee_bps_applied": fee_bps})
    }
}

impl Routr {

    pub(crate) fn deposit(&mut self, asset: AccountId, sender: AccountId, amount: U128) -> PromiseOrValue<U128> {
        let refuse = |why: &str| {
            emit("deposit_refused", json!({"account": sender, "asset": asset, "amount": amount, "reason": why}));
            PromiseOrValue::Value(amount)
        };
        if !Self::balance_assets().contains(&asset) {
            return refuse("asset");
        }
        let Some(mut a) = Self::account_get(&sender) else { return refuse("not_registered") };
        let prev = Self::balance_get(&sender, &asset);
        let held = Self::held_get(&asset);
        let (Some(bal), Some(total)) = (prev.unwrap_or(0).checked_add(amount.0), held.checked_add(amount.0)) else {
            return refuse("overflow");
        };
        let start = env::storage_usage();
        let had_held = env::storage_has_key(&key1(b"BHLD", asset.as_bytes()));
        Self::balance_put(&sender, &asset, bal);
        Self::held_put(&asset, total);
        let cost = env::storage_byte_cost().as_yoctonear() * env::storage_usage().saturating_sub(start) as u128;
        if cost > a.storage_credit {

            match prev {
                Some(v) => Self::balance_put(&sender, &asset, v),
                None => {
                    env::storage_remove(&key2(b"BBAL", sender.as_str(), asset.as_str()));
                }
            }
            if had_held {
                Self::held_put(&asset, held);
            } else {
                env::storage_remove(&key1(b"BHLD", asset.as_bytes()));
            }
            return refuse("storage");
        }
        a.storage_credit -= cost;
        Self::account_put(&sender, &a);
        emit("balance_deposit", json!({"account": sender, "asset": asset, "amount": amount, "balance": U128(bal)}));
        PromiseOrValue::Value(U128(0))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn place_sell(
        &mut self,
        token_in: AccountId,
        owner: AccountId,
        amount: U128,
        pool_id: String,
        min_out: U128,
        expires_ms: U64,
        tip: U128,
        builder: Option<AccountId>,
    ) -> PromiseOrValue<U128> {
        let id = self.resolve_pool(&pool_id);
        let p = self.seeded_pool(id);
        require!(token_in == p.token, "E_NOT_THE_TOKEN");

        require!(self.room_for(&p.platform_id), "E_STORAGE");

        require!(Self::balance_assets().contains(&p.quote), "E_QUOTE_NOT_BALANCE_ASSET");
        if let Some(b) = &builder {
            require!(*b != env::current_account_id(), "E_BUILDER");
        }
        check_order_terms(env::block_timestamp_ms(), amount.0, min_out.0, expires_ms.0).unwrap_or_else(|e| env::panic_str(e));
        require!(min_out.0.checked_add(tip.0).is_some(), "E_AMOUNT");
        let mut a = Self::account_get(&owner).unwrap_or_else(|| env::panic_str("E_NOT_REGISTERED"));
        let start = env::storage_usage();
        let held = Self::held_get(&token_in).checked_add(amount.0).unwrap_or_else(|| env::panic_str("E_AMOUNT"));
        Self::held_put(&token_in, held);
        Self::open_order(&owner, &mut a, start, OrderSide::Sell, &p, amount.0, min_out.0, expires_ms.0, tip.0, builder);
        PromiseOrValue::Value(U128(0))
    }

    #[allow(clippy::too_many_arguments)]
    fn open_order(
        who: &AccountId,
        a: &mut BalanceAccount,
        start: u64,
        side: OrderSide,
        p: &Pool,
        amount_in: u128,
        min_out: u128,
        expires_ms: u64,
        tip: u128,
        builder: Option<AccountId>,
    ) -> u64 {
        require!(a.orders.len() < MAX_OPEN_ORDERS, "E_TOO_MANY_ORDERS");
        let id = Self::next_order();
        env::storage_write(NEXT_ORDER_KEY, &(id + 1).to_le_bytes());
        let (token_in, token_out) = match side {
            OrderSide::Buy => (p.quote.clone(), p.token.clone()),
            OrderSide::Sell => (p.token.clone(), p.quote.clone()),
        };
        let mut o = Order {
            id: U64(id),
            owner: who.clone(),
            pool_id: p.id,
            side,
            token_in,
            token_out,
            amount_in: U128(amount_in),
            min_out: U128(min_out),
            tip: U128(tip),
            expires_ms: U64(expires_ms),
            created_ms: U64(env::block_timestamp_ms()),
            builder,
            storage_charged: U128(0),
        };
        Self::order_put(&o);
        a.orders.push(id);
        let cost = Self::account_commit(who, a, start, 0, true);
        o.storage_charged = U128(cost.max(0) as u128);
        Self::order_put(&o);
        emit("order_placed", json!(o));
        id
    }

    fn close_order(&mut self, o: &Order, refund: bool) {
        let owner = &o.owner;
        let mut a = Self::account_get(owner).unwrap_or_default();
        let escrow = Self::escrow_of(o);
        let start = env::storage_usage();
        env::storage_remove(&Self::order_key(o.id.0));
        a.orders.retain(|i| *i != o.id.0);
        if refund && o.side == OrderSide::Buy {

            let b = Self::balance_get(owner, &o.token_in).unwrap_or(0);
            Self::balance_put(owner, &o.token_in, b.saturating_add(escrow));
        } else {
            Self::held_put(&o.token_in, Self::held_get(&o.token_in).saturating_sub(escrow));
        }
        Self::account_commit(owner, &mut a, start, 0, false);
        if refund && o.side == OrderSide::Sell {

            let platform = self.pools.get(&o.pool_id.0).map(|v| v.get().platform_id.clone()).unwrap_or_default();
            self.payout(&o.token_in, owner, escrow, "refund", &platform);
        }
    }

    fn credit_tip(filler: &AccountId, asset: &AccountId, tip: u128) {
        if tip == 0 {
            return;
        }
        let mut a = Self::account_get(filler).unwrap_or_else(|| env::panic_str("E_FILLER_NOT_REGISTERED"));
        let start = env::storage_usage();
        Self::balance_put(filler, asset, Self::balance_get(filler, asset).unwrap_or(0).saturating_add(tip));
        Self::held_put(asset, Self::held_get(asset).saturating_add(tip));
        Self::account_commit(filler, &mut a, start, 0, true);
    }

    fn escrow_of(o: &Order) -> u128 {
        o.amount_in.0 + if o.side == OrderSide::Buy { o.tip.0 } else { 0 }
    }

    pub(crate) fn authorize(a: &mut BalanceAccount, spend: Option<(&AccountId, u128, &str)>) -> bool {
        let d = env::attached_deposit();
        if d == ONE_YOCTO {
            return false;
        }
        require!(d.is_zero(), "E_ONE_YOCTO");
        require!(env::signer_account_id() == env::predecessor_account_id(), "E_ONE_YOCTO");
        let pk = env::signer_account_pk();
        let now = env::block_timestamp_ms();
        match spend {
            None => require!(a.sessions.iter().any(|s| s.public_key == pk && now < s.expires_ms.0), "E_ONE_YOCTO"),
            Some((asset, amount, platform_id)) => {
                let known = a.sessions.iter().any(|s| s.public_key == pk);
                let Some(s) = a.sessions.iter_mut().find(|s| s.public_key == pk && &s.asset == asset) else {
                    env::panic_str(if known { "E_SESSION_ASSET" } else { "E_ONE_YOCTO" })
                };
                let (day, spent) = session_spend(s, now, platform_id, amount).unwrap_or_else(|e| env::panic_str(e));
                (s.day, s.spent) = (U64(day), U128(spent));
            }
        }
        true
    }

    fn seeded_pool(&self, id: u64) -> Pool {
        let p = self.pools.get(&id).map(|v| v.get().into_owned()).unwrap_or_else(|| env::panic_str("E_NO_POOL"));
        require!(p.seeded, "E_NOT_SEEDED");
        p
    }

    fn require_tradeable(&self, p: &Pool, side: OrderSide, amount_in: u128) {
        require!(self.room_for(&p.platform_id), "E_STORAGE");
        require!(side == OrderSide::Sell || p.real_quote.0.saturating_add(amount_in) <= RESERVE_MAX, "E_RESERVE");
    }

    pub(crate) fn payout(&mut self, token: &AccountId, to: &AccountId, amount: u128, why: &str, payer: &str) {
        if amount == 0 {
            return;
        }
        let key = (to.clone(), token.clone());
        if self.add_pending(&key, amount) {
            self.charge_key(payer);
            self.charge_key(payer);
            Self::pending_payer_set(&key, payer);
        }
        if self.add_owed_total(token, amount) {
            self.charge_key(payer);
        }
        self.deliver(token, to, amount, why, payer, None).detach();
    }

    pub(crate) fn account_commit(who: &AccountId, a: &mut BalanceAccount, start: u64, extra: i128, strict: bool) -> i128 {
        Self::account_put(who, a);
        let byte = env::storage_byte_cost().as_yoctonear() as i128;
        let cost = (env::storage_usage() as i128 - start as i128) * byte + extra;
        if cost > 0 {
            if strict {
                require!(a.storage_credit >= cost as u128, "E_STORAGE_CREDIT");
            }
            a.storage_credit = a.storage_credit.saturating_sub(cost as u128);
        } else {
            a.storage_credit = a.storage_credit.saturating_add(cost.unsigned_abs());
        }
        Self::account_put(who, a);
        cost
    }

    pub(crate) fn account_get(who: impl AsRef<str>) -> Option<BalanceAccount> {
        let v = env::storage_read(&key1(b"BACC", who.as_ref().as_bytes()))?;
        match near_sdk::borsh::from_slice::<VAccount>(&v).ok()? {
            VAccount::V1(a) => Some(a),
        }
    }

    pub(crate) fn account_put(who: impl AsRef<str>, a: &BalanceAccount) {
        env::storage_write(&key1(b"BACC", who.as_ref().as_bytes()), &near_sdk::borsh::to_vec(&VAccount::V1(a.clone())).unwrap());
    }

    fn balance_get(who: &AccountId, asset: &AccountId) -> Option<u128> {
        read_u128(&key2(b"BBAL", who.as_str(), asset.as_str()))
    }

    fn balance_put(who: &AccountId, asset: &AccountId, v: u128) {
        env::storage_write(&key2(b"BBAL", who.as_str(), asset.as_str()), &v.to_le_bytes());
    }

    fn held_get(token: &AccountId) -> u128 {
        read_u128(&key1(b"BHLD", token.as_bytes())).unwrap_or(0)
    }

    fn held_put(token: &AccountId, v: u128) {
        env::storage_write(&key1(b"BHLD", token.as_bytes()), &v.to_le_bytes());
    }

    pub(crate) fn balance_assets() -> Vec<AccountId> {
        env::storage_read(BALANCE_ASSETS_KEY)
            .and_then(|v| near_sdk::borsh::from_slice(&v).ok())
            .unwrap_or_default()
    }

    fn order_key(id: u64) -> Vec<u8> {
        key1(b"ORDR", &id.to_be_bytes())
    }

    fn order_get(id: u64) -> Option<Order> {
        let v = env::storage_read(&Self::order_key(id))?;
        match near_sdk::borsh::from_slice::<VOrder>(&v).ok()? {
            VOrder::V1(o) => Some(o),
        }
    }

    fn order_put(o: &Order) {
        env::storage_write(&Self::order_key(o.id.0), &near_sdk::borsh::to_vec(&VOrder::V1(o.clone())).unwrap());
    }

    fn next_order() -> u64 {
        env::storage_read(NEXT_ORDER_KEY)
            .and_then(|v| v.try_into().ok())
            .map_or(1, u64::from_le_bytes)
    }
}

