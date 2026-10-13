-- A GOG Galaxy 2.0 database in the shape a real galaxy-2.0.db had in
-- October 2026, read from its CREATE text, with
-- invented rows: no real ids, titles from the public catalogue only. The
-- tables UpNext reads are as Galaxy writes them, single quotes and all;
-- GamePieceTypes, InstalledBaseProducts and the four tables UpNext doesn't
-- read are close to the real ones but not copied word for word.
--
-- Next to galaxy-2.0.sql, the older shape described publicly: the reader
-- is tested against both.
CREATE TABLE 'Users'('id' INT64 NOT NULL PRIMARY KEY);
CREATE TABLE 'Platforms' (
	'id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	'name' TEXT NOT NULL,
	CONSTRAINT 'UK_Platforms_name' UNIQUE ('name'),
	CONSTRAINT 'CK_Platforms_name' CHECK(trim(name) <> '')
);
CREATE TABLE 'PlatformConnections'(
	'userId' INT64 NOT NULL,
	'platform' TEXT NOT NULL,
	'connectionState' TEXT NOT NULL,
	CONSTRAINT 'FK_PlatformConnections_userId_Users_id' FOREIGN KEY ('userId') REFERENCES 'Users' ('id') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'FK_PlatformConnections_platform_Platforms_name' FOREIGN KEY ('platform') REFERENCES 'Platforms' ('name') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'UK_PlatformConnections_userId_platform' UNIQUE ('userId', 'platform')
);
CREATE TABLE 'ReleaseKeys'(
	'key' TEXT NOT NULL PRIMARY KEY,
	CONSTRAINT 'CK_ReleaseKeys_key' CHECK(trim([key]) <> '' AND key LIKE '_%\_%_' ESCAPE '\')
);
CREATE TABLE 'LibraryReleases'(
	'id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	'userId' INT64 NOT NULL,
	'releaseKey' TEXT NOT NULL,
	CONSTRAINT 'FK_LibraryReleases_releaseKey_ReleaseKeys_key' FOREIGN KEY ('releaseKey') REFERENCES 'ReleaseKeys' ('key') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'FK_LibraryReleases_userId_Users_id' FOREIGN KEY ('userId') REFERENCES 'Users' ('id') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'UK_LibraryReleases_userId_key' UNIQUE ('userId', 'releaseKey')
);
CREATE TABLE 'LicensedReleases'(
	'libraryId' INTEGER NOT NULL,
	'isOwned' BOOLEAN NOT NULL,
	CONSTRAINT 'FK_LicensedReleases_libraryId_LibraryReleases_id' FOREIGN KEY ('libraryId') REFERENCES 'LibraryReleases' ('id') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'UK_LicensedReleases_libraryId' UNIQUE ('libraryId')
);
CREATE TABLE 'Subscriptions'(
	'id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	'name' TEXT NOT NULL,
	CONSTRAINT 'UK_Subscriptions_name' UNIQUE ('name')
);
CREATE TABLE 'SubscriptionReleases'(
	'id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	'subscriptionId' INTEGER NOT NULL,
	'licenseId' INTEGER NOT NULL,
	CONSTRAINT 'FK_SubscriptionReleases_subscriptionId_Subscriptions_id' FOREIGN KEY ('subscriptionId') REFERENCES 'Subscriptions' ('id') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'FK_SubscriptionReleases_subscriptionId_LicensedReleases_libraryId' FOREIGN KEY ('licenseId') REFERENCES 'LicensedReleases' ('libraryId') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'UK_SubscriptionReleases_subscriptionId_licenseId' UNIQUE ('subscriptionId', 'licenseId')
);
CREATE TABLE 'GamePieceTypes'(
	'id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	'type' TEXT NOT NULL,
	CONSTRAINT 'UK_GamePieceTypes_type' UNIQUE ('type'),
	CONSTRAINT 'CK_GamePieceTypes_type' CHECK(trim(type) <> '')
);
CREATE TABLE 'GamePieces'(
	'releaseKey' TEXT NOT NULL,
	'gamePieceTypeId' INTEGER NOT NULL,
	'userId' INT64 NULL,
	'value' TEXT NOT NULL,
	'languageId' INTEGER NULL
);
CREATE TABLE 'ReleaseProperties' (
	'releaseKey' TEXT NOT NULL,
	'isDlc' INTEGER NULL DEFAULT NULL,
	'isVisibleInLibrary' INTEGER NULL DEFAULT NULL,
	'gameId' TEXT NULL DEFAULT NULL,
	CONSTRAINT 'FK_ReleaseProperties_releaseKey_ReleaseKeys_key' FOREIGN KEY ('releaseKey') REFERENCES 'ReleaseKeys' ('key') ON DELETE CASCADE ON UPDATE CASCADE,
	CONSTRAINT 'UK_ReleaseProperties_gameReleaseKey' UNIQUE ('releaseKey')
);
CREATE TABLE 'UserReleaseProperties'(
	'userId' INT64 NOT NULL,
	'releaseKey' TEXT NOT NULL,
	'isHidden' BOOLEAN NULL,
	'hasTagsFetched' BOOLEAN NOT NULL DEFAULT 0
);
CREATE TABLE 'GameTimes'(
	'userId' INT64 NOT NULL,
	'releaseKey' TEXT NOT NULL,
	'minutesInGame' INTEGER NOT NULL
);
CREATE TABLE 'LastPlayedDates'(
	'userId' INT64 NOT NULL,
	'gameReleaseKey' TEXT NOT NULL,
	'lastPlayedDate' TEXT NOT NULL
);
CREATE TABLE 'ProductPurchaseDates'(
	'gameReleaseKey' TEXT NOT NULL,
	'userId' INT64 NOT NULL,
	'purchaseDate' TEXT NULL,
	'addedDate' TEXT NULL,
	CONSTRAINT 'UK_ProductPurchaseDates_gameReleaseKey_userId' UNIQUE ('gameReleaseKey', 'userId')
);
CREATE TABLE 'InstalledBaseProducts'(
	'productId' INTEGER NOT NULL PRIMARY KEY,
	'installationPath' TEXT NOT NULL,
	'installationDate' TEXT NULL,
	'generation' INTEGER NULL,
	'languageId' INTEGER NULL,
	'installationId' INTEGER NULL,
	'buildId' TEXT NULL,
	'branch' TEXT NULL
);
CREATE TABLE 'InstalledExternalProducts'(
	'id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	'platformId' INTEGER NOT NULL,
	'productId' TEXT NOT NULL,
	CONSTRAINT 'UK_InstalledExternalProducts_platformId_productId' UNIQUE ('platformId', 'productId')
);
CREATE TABLE 'ProductsToReleaseKeys'('externalId' INTEGER NULL, 'gogId' INTEGER NULL, 'releaseKey' TEXT NOT NULL);
CREATE TABLE 'AvailableLanguages'('id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT, 'code' TEXT NOT NULL);
CREATE TABLE 'InstalledProducts'('productId' INTEGER NOT NULL PRIMARY KEY, 'installationPath' TEXT NOT NULL);

INSERT INTO Users VALUES (48000000000000001);
INSERT INTO Platforms (id, name) VALUES (1, 'xboxone'), (3, 'epic'), (4, 'uplay'), (5, 'steam'), (7, 'origin'), (8, 'gog'), (86, 'eaApp');
-- Galaxy calls an unlinked integration "Connected" too (origin, xboxone).
INSERT INTO PlatformConnections VALUES
  (48000000000000001, 'epic', 'Connected'),
  (48000000000000001, 'steam', 'Connected'),
  (48000000000000001, 'origin', 'Connected'),
  (48000000000000001, 'xboxone', 'Connected'),
  (48000000000000001, 'uplay', 'Disconnected'),
  (48000000000000001, 'eaApp', 'Disconnected');
INSERT INTO GamePieceTypes (type) VALUES
  ('originalTitle'), ('title'), ('originalMeta'), ('meta'), ('originalImages'), ('summary'), ('media'),
  ('myRating'), ('osCompatibility'), ('sortingTitle'), ('originalSortingTitle'), ('allGameReleases');

INSERT INTO ReleaseKeys VALUES
  ('gog_1207658924'), ('steam_620'), ('steam_413150'), ('steam_1801110'), ('steam_480'), ('steam_397080'),
  ('epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c'), ('origin_71234'), ('origin_71235@subscription'), ('origin_71236'),
  ('xboxone_9n1234567890'), ('uplay_635');
INSERT INTO LibraryReleases (id, userId, releaseKey) VALUES
  (1, 48000000000000001, 'gog_1207658924'),
  (2, 48000000000000001, 'steam_620'),
  (3, 48000000000000001, 'steam_413150'),
  (4, 48000000000000001, 'steam_1801110'),
  (5, 48000000000000001, 'steam_480'),
  (6, 48000000000000001, 'steam_397080'),
  (7, 48000000000000001, 'epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c'),
  (8, 48000000000000001, 'origin_71234'),
  (9, 48000000000000001, 'origin_71235@subscription'),
  (10, 48000000000000001, 'origin_71236'),
  (11, 48000000000000001, 'xboxone_9n1234567890'),
  (12, 48000000000000001, 'uplay_635');
-- Steam Family Sharing: one shared game the player doesn't own (4), one
-- they own and also share (3). Release 7 has no licence row at all.
INSERT INTO LicensedReleases VALUES (1, 1), (2, 1), (3, 1), (4, 0), (5, 1), (6, 1), (8, 1), (9, 1), (10, 1), (11, 1), (12, 1);
INSERT INTO Subscriptions (id, name) VALUES (1, 'Steam Family Sharing');
INSERT INTO SubscriptionReleases (subscriptionId, licenseId) VALUES (1, 3), (1, 4);
INSERT INTO ReleaseProperties VALUES
  ('gog_1207658924', 0, 1, '1207658924'), ('steam_620', 0, 1, NULL), ('steam_413150', NULL, NULL, NULL);

INSERT INTO GamePieces (releaseKey, gamePieceTypeId, userId, value, languageId) VALUES
  ('gog_1207658924', 1, 48000000000000001, '{"title":"The Witcher: Enhanced Edition"}', NULL),
  ('gog_1207658924', 3, 48000000000000001, '{"developers":["CD PROJEKT RED"],"genres":["Role-playing"],"releaseDate":1221523200}', NULL),
  ('gog_1207658924', 5, 48000000000000001, '{"background":"https://images.gog.com/witcher_bg.jpg","squareIcon":null,"verticalCover":"https://images.gog.com/witcher_vc.jpg"}', NULL),
  ('steam_620', 1, 48000000000000001, '{"title":"Portal 2"}', NULL),
  ('steam_620', 5, 48000000000000001, '{"background":null,"squareIcon":null,"verticalCover":null}', NULL),
  ('steam_413150', 1, 48000000000000001, '{"title":"Stardew Valley"}', NULL),
  ('steam_1801110', 1, 48000000000000001, '{"title":"A Family Shared Game"}', NULL),
  ('steam_480', 1, 48000000000000001, '{"title":"Spacewar"}', NULL),
  ('steam_397080', 1, 48000000000000001, '{"title":"Magicka 2 Spell Balance Beta"}', NULL),
  ('epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c', 1, 48000000000000001, '{"title":"Hollow Hills"}', NULL),
  ('origin_71234', 1, 48000000000000001, '{"title":"Mass Effect Legendary Edition"}', NULL),
  ('origin_71235@subscription', 1, 48000000000000001, '{"title":"Battlefield 4 Premium Edition"}', NULL),
  ('origin_71236', 1, 48000000000000001, '{"title":"Battlefield 2042 Open Beta"}', NULL),
  ('xboxone_9n1234567890', 1, 48000000000000001, '{"title":"Gears 5"}', NULL),
  ('uplay_635', 1, 48000000000000001, '{"title":"Far Cry 3"}', NULL);

INSERT INTO GameTimes VALUES (48000000000000001, 'gog_1207658924', 754), (48000000000000001, 'steam_620', 90);
-- Mostly an offset without a colon; a few plain date-times.
INSERT INTO LastPlayedDates VALUES
  (48000000000000001, 'gog_1207658924', '2026-08-01T22:15:00+0200'),
  (48000000000000001, 'steam_620', '2026-07-04 18:00:00');
INSERT INTO ProductPurchaseDates VALUES
  ('gog_1207658924', 48000000000000001, '2019-06-01T10:00:00+0000', '2019-06-01T10:00:00+0000'),
  ('steam_620', 48000000000000001, NULL, '2026-10-01T08:00:00+0000'),
  ('steam_413150', 48000000000000001, NULL, '2026-09-30T08:00:00+0000'),
  ('epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c', 48000000000000001, NULL, '2026-10-02T08:00:00+0000'),
  ('origin_71234', 48000000000000001, NULL, '2024-05-18T12:00:00+0000'),
  ('origin_71236', 48000000000000001, NULL, '2021-10-06T12:00:00+0000'),
  ('xboxone_9n1234567890', 48000000000000001, NULL, '2023-03-15T12:00:00+0000');

INSERT INTO InstalledBaseProducts (productId, installationPath) VALUES (1207658924, 'C:\GOG Games\The Witcher');
INSERT INTO InstalledExternalProducts (platformId, productId) VALUES (5, '620');
