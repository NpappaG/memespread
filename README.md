# memespread

**memespread** was born to examine with extreme precision just how much supply of a coin is concentrated and how this concentration changes over time. Using ClickHouse's powerful materialized views and real-time data processing, it provides granular insights into token distribution patterns, helping you understand the true concentration dynamics of any Solana token.

## Setup

### Option 1: Local Development

1. Add `.env` file with your Helius API key:

   ```
   HELIUS_API_KEY=XXXXX-xxxx-XXXX
   ```

2. Install ClickHouse (if not already installed):

   ```bash
   # On macOS
   brew install --cask clickhouse

   ```

   For more installation options, see the [ClickHouse installation docs](https://clickhouse.com/docs/install).

3. In one terminal, first start the ClickHouse DB server:

   ```bash
   clickhouse-server
   ```

4. In a second terminal, run the application in debug mode:

   ```bash
   cargo run debug=true
   ```

   The application will automatically connect to the "default" ClickHouse database and initialize all necessary tables and materialized views on startup.

5. **Verify Setup**: After startup, you can verify the database was created correctly by visiting `http://localhost:8123/play` in your browser. This opens ClickHouse's web interface where you can run `SHOW TABLES;` to see all the created tables.

### Option 2: Docker

1. Add `.env` file with your Helius API key:

   ```
   HELIUS_API_KEY=XXXXX-xxxx-XXXX
   ```

2. Run with Docker Compose:

   ```bash
   docker-compose up --build
   ```

3. **Verify Setup**: After startup, you can verify the database was created correctly by:

   - Visit `http://localhost:8123/play` in your browser to access ClickHouse's web interface
   - Run `SHOW TABLES;` to see all created tables
   - Run `SELECT 1;` to verify the connection is working

## Usage

**Browser Interface**: You can interact with the API directly in your browser by visiting the URL above. (Or GET the endpoint below with an app) You may want to build a frontend application to provide a more polished user experience for querying and visualizing token concentration data.

### Adding a Coin to the Clickhouse DB for Monitoring

To add a coin to the database, navigate to:

```
http://localhost:8000/token-stats?mint_address={solcontractaddress}
```

Replace `{solcontractaddress}` with the actual Solana contract address / mint address (only SPL tokens currently).

**First Visit**: If the coin is not already being monitored, that mint addrss will be added to the monitoring system.

You can also query `SELECT * FROM monitored_tokens;` to see which tokens are being monitored - monitoring updates occur every minute by default.

Wait 1-2 minutes to populate Clickhouse with enough observations.

**Subsequent Visits**: After a coin has been monitored once or twice, going to `http://localhost:3000/token-stats?mint_address={solcontractaddress}` will now return:

- **Concentration Metrics**: Token supply percentages owned by the largest N wallets of the coin (1, 10, 25, 50, 100, 250 holders)
- **Distribution Stats**: HHI score, distribution score, balance statistics
- **Holder Thresholds**: Breakdown of holder count by various USD value thresholds ($10, $100, $1K, $10K, $100K) of a given coin (at current market prices).
- **Token Stats**: Market cap, price, supply, decimals

These are calculated using the power of Materialized Views in Clickhouse.

### API response

Example response for a monitored coin with a few observations:

```json
{
  "concentration_metrics": [
    {
      "percentage": 3.603,
      "top_n": 1
    },
    {
      "percentage": 17.0322,
      "top_n": 10
    }
    ...
  ]
  "distribution_stats": {
    "distribution_score": 2.6685,
    "hhi": 60.0966,
    "mean_balance": 0,
    "median_balance": 0,
    "total_count": 0
  },
  "holder_thresholds": [
    {
      "holder_count": 3785,
      "mcap_per_holder": 3538528050.33,
      "pct_of_10usd": 100,
      "pct_total_holders": 50.3124,
      "slice_value_usd": 13369938.38,
      "total_holders": 7523,
      "usd_threshold": 10
    }
  ],
  "token_stats": {
    "decimals": 6,
    "market_cap": 13400937426424,
    "price": 0.013403055,
    "supply": 999842025249964
  }
}
```

## Database Management

If there's problems with the app adding data (eg an invalid mint address), you may need to hand-edit the database. There's a few methods to connect to your Clickhouse DB:

#### In-browser (recommended)

```bash
http://localhost:8123/play
```

#### Local dev: in-terminal

```bash
# Connect to ClickHouse client
clickhouse-client
```

# Docker: in-terminal

```
docker exec -it <container_name> clickhouse-client
```

#### Common commands

Test if DB has ~14 default tables:

```sql
-- List tables
SHOW TABLES;
```

Remove a token from monitoring:

```sql
-- Delete a token from monitoring
DELETE FROM monitored_tokens WHERE mint_address = 'your_token_address';
```

Nuclear option to kill entire default db [Warning: deletes data - restart app/Docker container to re-make db]

```sql
-- Clear all data for fresh start (restart app)
DROP DATABASE DEFAULT;
```

Then:

```sql
-- Clear all data for fresh start (restart app)
CREATE DATABASE DEFAULT;
```
