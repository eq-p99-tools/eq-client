//! How an installed client prices what its merchant window shows, from the
//! program. A client generation's rules are its [`MerchantPrices`], from
//! [`crate::ui::InstalledClient::merchant_prices`]; a generation whose rules
//! are not yet read has none.
//!
//! The Mac-era rules are from the TAKP client's executable, read statically
//! on 10-30 (TAKP Batch 1 R7, recording checklist item 6.170).

/// How a client generation prices a merchant's wares and the player's
/// items, and the lines it says them in, by their ids in its string table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MerchantPrices {
    /// The lines for a price's platinum, gold, silver and copper, each
    /// with its count; a part that is zero is left out.
    pub coins: [u32; 4],
    /// What a ware with no list price shows.
    pub no_price: u32,
    /// What an item the merchant would pay nothing for shows.
    pub no_value: u32,
    /// The refusal of a buy the coins on the player don't cover: of one,
    /// and of more.
    pub short: [u32; 2],
    /// The received list price already includes the merchant rate.
    pub listed_buy: bool,
    /// Separator between the installed coin fragments.
    pub coin_separator: &'static str,
    /// Native received-sale wording is read for this generation.
    pub says_sale: bool,
}

/// From this sale total up, the program rounds it to the nearest 100.
const ROUNDED_SALE: u64 = 10_000_000;

impl MerchantPrices {
    /// What `quantity` of a ware with this list price cost, in copper.
    /// Titanium uses its already-rated listed price; the Mac-era path uses
    /// the rate the merchant opened with: trunc(quantity × price × rate +
    /// 0.5), rounded once after the quantity. A negative or zero price costs
    /// nothing. The program multiplies by an item field (+0x120) where it
    /// is above 1 for an item of class 0; the sessions don't decode that
    /// field, so it counts as absent (unsure).
    #[must_use]
    pub fn buy_total(&self, price: u32, rate: f32, quantity: u32) -> u64 {
        if self.listed_buy {
            return u64::from(price) * u64::from(quantity);
        }
        if price == 0 || !rate.is_finite() || rate <= 0.0 {
            return 0;
        }
        let each = f64::from(price) * f64::from(rate);
        // A price past 2^53 copper is no price the servers send.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let total = (f64::from(quantity) * each + 0.5) as u64;
        total
    }

    /// What the merchant pays for `quantity` of an item with this list
    /// price, in copper: each unit trunc(price / rate + 0.5), then times the
    /// quantity, and from 10,000,000 rounded to the nearest 100.
    #[must_use]
    pub fn sale_total(&self, price: u32, rate: f32, quantity: u32) -> u64 {
        let total = self
            .sale_unit(price, rate)
            .saturating_mul(u64::from(quantity));
        if total >= ROUNDED_SALE {
            let rounded = total.saturating_add(50);
            rounded - rounded % 100
        } else {
            total
        }
    }

    /// What the merchant pays for one unit of an item with this list price;
    /// an item worth less than 1 copper can't be sold.
    #[must_use]
    pub fn sale_unit(&self, price: u32, rate: f32) -> u64 {
        if price == 0 || !rate.is_finite() || rate <= 0.0 {
            return 0;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let unit = (f64::from(price) / f64::from(rate) + 0.5) as u64;
        unit
    }

    /// A price's coins, from platinum down to copper, as the program
    /// splits it: platinum the thousands, then gold, silver and copper.
    #[must_use]
    pub const fn split(copper: u64) -> [u64; 4] {
        [
            copper / 1000,
            copper % 1000 / 100,
            copper % 100 / 10,
            copper % 10,
        ]
    }
}

/// The Mac-era client's merchant prices (from the program, 6.170).
pub const EQMAC: MerchantPrices = MerchantPrices {
    coins: [2409, 2410, 2411, 2412],
    no_price: 2413,
    no_value: 2408,
    short: [12060, 12061],
    listed_buy: false,
    coin_separator: " ",
    says_sale: false,
};

/// Titanium: sale quote 0x5e1690 (each price / f32 rate + 0.5,
/// truncated, then quantity and large-total rounding), price text 0x46daaf.
/// Buy text 0x4b1d79 uses the listed instance price without the rate again.
pub const TITANIUM: MerchantPrices = MerchantPrices {
    coins: [0x969, 0x96a, 0x96b, 0x96c],
    no_price: 0x96d,
    no_value: 0x968,
    short: [12060, 12061],
    listed_buy: true,
    coin_separator: "",
    says_sale: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titanium_uses_the_retained_sale_quote_without_repricing_listed_buys() {
        // Retained 6.704 witness: Bat Fur base15, accepted window rate,
        // then received sale quantity1/price14. It is not a quote response.
        let rate = f32::from_bits(0x3f86_bca2);
        assert_eq!(TITANIUM.sale_total(15, rate, 1), 14);
        assert_eq!(TITANIUM.sale_total(15, rate, 2), 28);
        assert_eq!(TITANIUM.buy_total(15, rate, 2), 30);
        assert_eq!(TITANIUM.sale_total(10_000_050, 1.0, 1), 10_000_100);
        assert_eq!(TITANIUM.sale_total(0, rate, 1), 0);
    }

    #[test]
    fn a_buy_rounds_once_after_the_quantity() {
        // 10 × 1.25 = 12.5 each: 13 for one, 25 for two (not 26).
        assert_eq!(EQMAC.buy_total(10, 1.25, 1), 13);
        assert_eq!(EQMAC.buy_total(10, 1.25, 2), 25);
        // The rate is a 32-bit float: 1.05 is a hair under, so 10.5 is too.
        assert_eq!(EQMAC.buy_total(10, 1.05, 1), 10);
        assert_eq!(EQMAC.buy_total(0, 1.25, 3), 0);
        assert_eq!(EQMAC.buy_total(10, f32::NAN, 1), 0);
    }

    #[test]
    fn a_sale_rounds_each_unit_and_large_totals_to_a_hundred() {
        // 10 / 0.75 = 13.3 each: 13 a unit, 26 for two.
        assert_eq!(EQMAC.sale_unit(10, 0.75), 13);
        assert_eq!(EQMAC.sale_total(10, 0.75, 2), 26);
        // 0.4 rounds to nothing: not for sale.
        assert_eq!(EQMAC.sale_unit(1, 2.5), 0);
        assert_eq!(EQMAC.sale_total(10_000_049, 1.0, 1), 10_000_000);
        assert_eq!(EQMAC.sale_total(10_000_050, 1.0, 1), 10_000_100);
        assert_eq!(EQMAC.sale_total(9_999_999, 1.0, 1), 9_999_999);
    }

    #[test]
    fn a_price_splits_into_its_coins() {
        assert_eq!(MerchantPrices::split(12_345), [12, 3, 4, 5]);
    }
}
