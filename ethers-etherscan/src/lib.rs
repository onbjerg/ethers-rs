//! Bindings for [etherscan.io web api](https://docs.etherscan.io/)

use std::{borrow::Cow, io::Write, path::PathBuf};

use contract::ContractMetadata;
use reqwest::{header, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use errors::EtherscanError;
use ethers_core::{
    abi::{Abi, Address},
    types::Chain,
};

pub mod account;
pub mod contract;
pub mod errors;
pub mod gas;
pub mod source_tree;
pub mod transaction;

pub(crate) type Result<T> = std::result::Result<T, EtherscanError>;

/// The Etherscan.io API client.
#[derive(Clone, Debug)]
pub struct Client {
    /// Client that executes HTTP requests
    client: reqwest::Client,
    /// Etherscan API key
    api_key: String,
    /// Etherscan API endpoint like <https://api(-chain).etherscan.io/api>
    etherscan_api_url: Url,
    /// Etherscan base endpoint like <https://etherscan.io>
    etherscan_url: Url,
    /// Path to where ABI files should be cached
    cache: Option<Cache>,
}

#[derive(Clone, Debug)]
// Simple cache for etherscan requests
struct Cache(PathBuf);

impl Cache {
    fn get_abi(&self, address: Address) -> Result<Option<ethers_core::abi::Abi>> {
        self.get("abi", address)
    }

    fn set_abi(&self, address: Address, abi: &Abi) -> Result<()> {
        self.set("abi", address, abi)
    }

    fn get_source(&self, address: Address) -> Result<Option<ContractMetadata>> {
        self.get("sources", address)
    }

    fn set_source(&self, address: Address, source: &ContractMetadata) -> Result<()> {
        self.set("sources", address, source)
    }

    fn set<T: Serialize>(&self, prefix: &str, address: Address, item: T) -> Result<()> {
        let path = self.0.join(prefix).join(format!("{:?}.json", address));
        let mut writer = std::io::BufWriter::new(std::fs::File::create(path)?);
        serde_json::to_writer(&mut writer, &item)?;
        // TODO: Trace
        // TODO: Should we cache if the contract is *not* verified?
        let _ = writer.flush();
        Ok(())
    }

    fn get<T: DeserializeOwned>(&self, prefix: &str, address: Address) -> Result<Option<T>> {
        let path = self.0.join(prefix).join(format!("{:?}.json", address));
        let reader = std::io::BufReader::new(std::fs::File::create(path)?);
        if let Ok(inner) = serde_json::from_reader(reader) {
            return Ok(Some(inner))
        }
        Ok(None)
    }
}

impl Client {
    pub fn new_cached(
        chain: Chain,
        api_key: impl Into<String>,
        cache: Option<PathBuf>,
    ) -> Result<Self> {
        let mut this = Self::new(chain, api_key)?;
        this.cache = cache.map(Cache);
        Ok(this)
    }

    /// Create a new client with the correct endpoints based on the chain and provided API key
    pub fn new(chain: Chain, api_key: impl Into<String>) -> Result<Self> {
        let (etherscan_api_url, etherscan_url) = match chain {
            Chain::Mainnet => {
                (Url::parse("https://api.etherscan.io/api"), Url::parse("https://etherscan.io"))
            }
            Chain::Ropsten | Chain::Kovan | Chain::Rinkeby | Chain::Goerli => {
                let chain_name = chain.to_string().to_lowercase();

                (
                    Url::parse(&format!("https://api-{}.etherscan.io/api", chain_name)),
                    Url::parse(&format!("https://{}.etherscan.io", chain_name)),
                )
            }
            Chain::Polygon => (
                Url::parse("https://api.polygonscan.com/api"),
                Url::parse("https://polygonscan.com"),
            ),
            Chain::PolygonMumbai => (
                Url::parse("https://api-testnet.polygonscan.com/api"),
                Url::parse("https://mumbai.polygonscan.com"),
            ),
            Chain::Avalanche => {
                (Url::parse("https://api.snowtrace.io/api"), Url::parse("https://snowtrace.io"))
            }
            Chain::AvalancheFuji => (
                Url::parse("https://api-testnet.snowtrace.io/api"),
                Url::parse("https://testnet.snowtrace.io"),
            ),
            Chain::Optimism => (
                Url::parse("https://api-optimistic.etherscan.io/api"),
                Url::parse("https://optimistic.etherscan.io"),
            ),
            Chain::OptimismKovan => (
                Url::parse("https://api-kovan-optimistic.etherscan.io/api"),
                Url::parse("https://kovan-optimistic.etherscan.io"),
            ),
            Chain::Fantom => {
                (Url::parse("https://api.ftmscan.com"), Url::parse("https://ftmscan.com"))
            }
            Chain::FantomTestnet => (
                Url::parse("https://api-testnet.ftmscan.com"),
                Url::parse("https://testnet.ftmscan.com"),
            ),
            Chain::BinanceSmartChain => {
                (Url::parse("https://api.bscscan.com/api"), Url::parse("https://bscscan.com"))
            }
            Chain::BinanceSmartChainTestnet => (
                Url::parse("https://api-testnet.bscscan.com/api"),
                Url::parse("https://testnet.bscscan.com"),
            ),
            Chain::Arbitrum => {
                (Url::parse("https://api.arbiscan.io/api"), Url::parse("https://arbiscan.io"))
            }
            Chain::ArbitrumTestnet => (
                Url::parse("https://api-testnet.arbiscan.io/api"),
                Url::parse("https://testnet.arbiscan.io"),
            ),
            Chain::Cronos => {
                (Url::parse("https://api.cronoscan.com/api"), Url::parse("https://cronoscan.com"))
            }
            Chain::Dev => return Err(EtherscanError::LocalNetworksNotSupported),
            chain => return Err(EtherscanError::ChainNotSupported(chain)),
        };

        Ok(Self {
            client: Default::default(),
            api_key: api_key.into(),
            etherscan_api_url: etherscan_api_url.expect("is valid http"),
            etherscan_url: etherscan_url.expect("is valid http"),
            cache: None,
        })
    }

    /// Create a new client with the correct endpoints based on the chain and API key
    /// from ETHERSCAN_API_KEY environment variable
    pub fn new_from_env(chain: Chain) -> Result<Self> {
        let api_key = match chain {
            Chain::Avalanche | Chain::AvalancheFuji => std::env::var("SNOWTRACE_API_KEY")?,
            Chain::Polygon | Chain::PolygonMumbai => std::env::var("POLYGONSCAN_API_KEY")?,
            Chain::Mainnet |
            Chain::Ropsten |
            Chain::Kovan |
            Chain::Rinkeby |
            Chain::Goerli |
            Chain::Optimism |
            Chain::OptimismKovan |
            Chain::Fantom |
            Chain::FantomTestnet |
            Chain::BinanceSmartChain |
            Chain::BinanceSmartChainTestnet |
            Chain::Arbitrum |
            Chain::ArbitrumTestnet |
            Chain::Cronos => std::env::var("ETHERSCAN_API_KEY")?,

            Chain::XDai | Chain::Sepolia | Chain::CronosTestnet => String::default(),
            Chain::Moonbeam | Chain::MoonbeamDev | Chain::Moonriver => {
                std::env::var("MOONSCAN_API_KEY")?
            }
            Chain::Dev => return Err(errors::EtherscanError::LocalNetworksNotSupported),
        };
        Self::new(chain, api_key)
    }

    pub fn etherscan_api_url(&self) -> &Url {
        &self.etherscan_api_url
    }

    pub fn etherscan_url(&self) -> &Url {
        &self.etherscan_url
    }

    /// Return the URL for the given block number
    pub fn block_url(&self, block: u64) -> String {
        format!("{}block/{}", self.etherscan_url, block)
    }

    /// Return the URL for the given address
    pub fn address_url(&self, address: Address) -> String {
        format!("{}address/{}", self.etherscan_url, address)
    }

    /// Return the URL for the given transaction hash
    pub fn transaction_url(&self, tx_hash: impl AsRef<str>) -> String {
        format!("{}tx/{}", self.etherscan_url, tx_hash.as_ref())
    }

    /// Return the URL for the given token hash
    pub fn token_url(&self, token_hash: impl AsRef<str>) -> String {
        format!("{}token/{}", self.etherscan_url, token_hash.as_ref())
    }

    /// Execute an API POST request with a form
    async fn post_form<T: DeserializeOwned, Form: Serialize>(
        &self,
        form: &Form,
    ) -> Result<Response<T>> {
        Ok(self
            .client
            .post(self.etherscan_api_url.clone())
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(form)
            .send()
            .await?
            .json()
            .await?)
    }

    /// Execute an API GET request with parameters
    async fn get_json<T: DeserializeOwned, Q: Serialize>(&self, query: &Q) -> Result<Response<T>> {
        Ok(self
            .client
            .get(self.etherscan_api_url.clone())
            .header(header::ACCEPT, "application/json")
            .query(query)
            .send()
            .await?
            .json()
            .await?)
    }

    fn create_query<T: Serialize>(
        &self,
        module: &'static str,
        action: &'static str,
        other: T,
    ) -> Query<T> {
        Query {
            apikey: Cow::Borrowed(&self.api_key),
            module: Cow::Borrowed(module),
            action: Cow::Borrowed(action),
            other,
        }
    }
}

/// The API response type
#[derive(Debug, Clone, Deserialize)]
pub struct Response<T> {
    pub status: String,
    pub message: String,
    pub result: T,
}

/// The type that gets serialized as query
#[derive(Debug, Serialize)]
struct Query<'a, T: Serialize> {
    apikey: Cow<'a, str>,
    module: Cow<'a, str>,
    action: Cow<'a, str>,
    #[serde(flatten)]
    other: T,
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        time::{Duration, SystemTime},
    };

    use ethers_core::types::{Address, Chain};

    use crate::{Client, EtherscanError};

    #[test]
    fn chain_not_supported() {
        let err = Client::new_from_env(Chain::XDai).unwrap_err();

        assert!(matches!(err, EtherscanError::ChainNotSupported(_)));
        assert_eq!(err.to_string(), "chain xdai not supported");
    }

    #[test]
    fn stringifies_block_url() {
        let etherscan = Client::new_from_env(Chain::Mainnet).unwrap();
        let block: u64 = 1;
        let block_url: String = etherscan.block_url(block);
        assert_eq!(block_url, format!("https://etherscan.io/block/{}", block));
    }

    #[test]
    fn stringifies_address_url() {
        let etherscan = Client::new_from_env(Chain::Mainnet).unwrap();
        let addr: Address = Address::zero();
        let address_url: String = etherscan.address_url(addr);
        assert_eq!(address_url, format!("https://etherscan.io/address/{}", addr));
    }

    #[test]
    fn stringifies_transaction_url() {
        let etherscan = Client::new_from_env(Chain::Mainnet).unwrap();
        let tx_hash = "0x0";
        let tx_url: String = etherscan.transaction_url(tx_hash);
        assert_eq!(tx_url, format!("https://etherscan.io/tx/{}", tx_hash));
    }

    #[test]
    fn stringifies_token_url() {
        let etherscan = Client::new_from_env(Chain::Mainnet).unwrap();
        let token_hash = "0x0";
        let token_url: String = etherscan.token_url(token_hash);
        assert_eq!(token_url, format!("https://etherscan.io/token/{}", token_hash));
    }

    #[test]
    fn local_networks_not_supported() {
        let err = Client::new_from_env(Chain::Dev).unwrap_err();
        assert!(matches!(err, EtherscanError::LocalNetworksNotSupported));
    }

    pub async fn run_at_least_duration(duration: Duration, block: impl Future) {
        let start = SystemTime::now();
        block.await;
        if let Some(sleep) = duration.checked_sub(start.elapsed().unwrap()) {
            tokio::time::sleep(sleep).await;
        }
    }
}
