CREATE TABLE bookings (
    id SERIAL PRIMARY KEY,
    booking_id VARCHAR NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    last_modified_at TIMESTAMPTZ NOT NULL,
    cancelled_at TIMESTAMPTZ DEFAULT NULL,
    type_ VARCHAR NOT NULL DEFAULT '',
    status VARCHAR NOT NULL DEFAULT '',
    vendor VARCHAR NOT NULL DEFAULT '',
    flight VARCHAR NOT NULL DEFAULT '',
    cabin VARCHAR NOT NULL DEFAULT '',
    is_preferred_vendor BOOLEAN NOT NULL DEFAULT 'f',
    used_corporate_discount BOOLEAN NOT NULL DEFAULT 'f',
    start_date DATE NOT NULL,
    end_date DATE DEFAULT NULL,
    passengers TEXT [] NOT NULL,
    booker VARCHAR NOT NULL DEFAULT '',
    origin VARCHAR NOT NULL DEFAULT '',
    destination VARCHAR NOT NULL DEFAULT '',
    length VARCHAR NOT NULL DEFAULT '',
    description VARCHAR NOT NULL DEFAULT '',
    currency VARCHAR NOT NULL DEFAULT '',
    optimal_price REAL NOT NULL DEFAULT 0,
    grand_total REAL NOT NULL DEFAULT 0,
    purpose VARCHAR NOT NULL DEFAULT '',
    reason VARCHAR NOT NULL DEFAULT '',
    confirmation_id VARCHAR NOT NULL DEFAULT '',
    cio_company_id INTEGER NOT NULL DEFAULT 0,
    airtable_record_id VARCHAR NOT NULL DEFAULT ''
)
