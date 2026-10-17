use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use thiserror::Error;
pub const MICRO_CREDITS_PER_CREDIT: u64 = 1_000_000;
pub const MAX_SPLIT_BPS: u32 = 10_000;
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("insufficient balance: needed {needed}, available {available}")]
    InsufficientBalance { needed: u64, available: u64 },
    #[error("buyer and seller cannot be the same")]
    SelfPurchase,
    #[error("invalid split basis points: {0}")]
    InvalidSplit(u32),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}
#[derive(Debug)]
pub struct Settlement {
    pub seller_micro: u64,
    pub platform_micro: u64,
}
pub struct Ledger {
    conn: Mutex<Connection>,
}
impl Ledger {
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }
    pub fn open_in_memory() -> Result<Self, LedgerError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }
    fn init(conn: Connection) -> Result<Self, LedgerError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS market_balances (
                account_id TEXT PRIMARY KEY,
                micro_credits INTEGER NOT NULL CHECK (micro_credits >= 0)
            )",
            [],
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn check_account(account: &str) -> Result<(), LedgerError> {
        if account.trim().is_empty() {
            Err(LedgerError::InvalidInput(
                "account cannot be blank".to_string(),
            ))
        } else {
            Ok(())
        }
    }
    pub fn credit(&self, account: &str, micro: u64) -> Result<(), LedgerError> {
        Self::check_account(account)?;
        self.conn().execute("INSERT INTO market_balances (account_id, micro_credits) VALUES (?1, ?2) ON CONFLICT(account_id) DO UPDATE SET micro_credits = micro_credits + ?2", params![account, micro as i64])?;
        Ok(())
    }
    pub fn balance(&self, account: &str) -> Result<u64, LedgerError> {
        Self::check_account(account)?;
        let micro: Option<i64> = self
            .conn()
            .query_row(
                "SELECT micro_credits FROM market_balances WHERE account_id = ?1",
                params![account],
                |row| row.get(0),
            )
            .optional()?;
        Ok(micro.unwrap_or(0) as u64)
    }
    pub fn purchase(
        &self,
        buyer: &str,
        seller: &str,
        price_micro: u64,
        split_bps: u32,
    ) -> Result<Settlement, LedgerError> {
        if buyer == seller {
            return Err(LedgerError::SelfPurchase);
        }
        if split_bps > MAX_SPLIT_BPS {
            return Err(LedgerError::InvalidSplit(split_bps));
        }
        Self::check_account(buyer)?;
        Self::check_account(seller)?;
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let buyer_bal: Option<i64> = tx
            .query_row(
                "SELECT micro_credits FROM market_balances WHERE account_id = ?1",
                params![buyer],
                |row| row.get(0),
            )
            .optional()?;
        let buyer_bal = buyer_bal.unwrap_or(0) as u64;
        if buyer_bal < price_micro {
            return Err(LedgerError::InsufficientBalance {
                needed: price_micro,
                available: buyer_bal,
            });
        }
        let platform_micro = ((price_micro as u128 * split_bps as u128) / 10_000) as u64;
        let seller_micro = price_micro - platform_micro;
        tx.execute(
            "UPDATE market_balances SET micro_credits = micro_credits - ?2 WHERE account_id = ?1",
            params![buyer, price_micro as i64],
        )?;
        tx.execute(
            "INSERT INTO market_balances (account_id, micro_credits)
             VALUES (?1, ?2)
             ON CONFLICT(account_id) DO UPDATE SET micro_credits = micro_credits + ?2",
            params![seller, seller_micro as i64],
        )?;
        tx.commit()?;
        Ok(Settlement {
            seller_micro,
            platform_micro,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn purchase_debits_buyer_and_credits_seller_minus_split() {
        let ledger = Ledger::open_in_memory().unwrap();
        ledger.credit("buyer", 1000).unwrap();
        let settlement = ledger.purchase("buyer", "seller", 1000, 1000).unwrap();
        assert_eq!(settlement.platform_micro, 100);
        assert_eq!(settlement.seller_micro, 900);
        assert_eq!(ledger.balance("buyer").unwrap(), 0);
        assert_eq!(ledger.balance("seller").unwrap(), 900);
    }
    #[test]
    fn insufficient_balance_fails_and_balances_are_unchanged() {
        let ledger = Ledger::open_in_memory().unwrap();
        ledger.credit("buyer", 500).unwrap();
        let err = ledger.purchase("buyer", "seller", 1000, 1000).unwrap_err();
        assert!(matches!(
            err,
            LedgerError::InsufficientBalance {
                needed: 1000,
                available: 500
            }
        ));
        assert_eq!(ledger.balance("buyer").unwrap(), 500);
        assert_eq!(ledger.balance("seller").unwrap(), 0);
    }
    #[test]
    fn self_purchase_refused() {
        let ledger = Ledger::open_in_memory().unwrap();
        ledger.credit("me", 1000).unwrap();
        let err = ledger.purchase("me", "me", 1000, 1000).unwrap_err();
        assert!(matches!(err, LedgerError::SelfPurchase));
    }
    #[test]
    fn split_above_10000_refused() {
        let ledger = Ledger::open_in_memory().unwrap();
        let err = ledger.purchase("buyer", "seller", 1000, 10001).unwrap_err();
        assert!(matches!(err, LedgerError::InvalidSplit(10001)));
    }
    #[test]
    fn blank_account_refused() {
        let ledger = Ledger::open_in_memory().unwrap();
        assert!(matches!(
            ledger.credit("  ", 1000),
            Err(LedgerError::InvalidInput(_))
        ));
        assert!(matches!(
            ledger.balance(""),
            Err(LedgerError::InvalidInput(_))
        ));
        assert!(matches!(
            ledger.purchase("buyer", "  ", 1000, 1000),
            Err(LedgerError::InvalidInput(_))
        ));
        assert!(matches!(
            ledger.purchase("  ", "seller", 1000, 1000),
            Err(LedgerError::InvalidInput(_))
        ));
    }
    #[test]
    fn purchase_with_split_zero_credits_full_price() {
        let ledger = Ledger::open_in_memory().unwrap();
        ledger.credit("buyer", 1000).unwrap();
        let settlement = ledger.purchase("buyer", "seller", 1000, 0).unwrap();
        assert_eq!(settlement.platform_micro, 0);
        assert_eq!(settlement.seller_micro, 1000);
        assert_eq!(ledger.balance("buyer").unwrap(), 0);
        assert_eq!(ledger.balance("seller").unwrap(), 1000);
    }
}
