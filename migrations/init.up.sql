CREATE TABLE bot_images (
    hash text NOT NULL,
    instances integer NOT NULL,
    first_seen timestamp with time zone NOT NULL,
    last_seen timestamp with time zone NOT NULL
);

ALTER TABLE ONLY bot_images
    ADD CONSTRAINT bot_images_pkey PRIMARY KEY (hash);