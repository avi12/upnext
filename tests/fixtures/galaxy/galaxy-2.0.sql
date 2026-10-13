-- A GOG Galaxy 2.0 database in the shape described publicly (the community
-- export scripts and schema dumps), with invented rows: the tables UpNext
-- reads, their other columns, and a few tables it doesn't read. Not yet
-- compared with a real galaxy-2.0.db; replace the CREATE statements with
-- `sqlite3 galaxy-2.0.db .schema` output from a Windows computer with GOG Galaxy.
CREATE TABLE "GamePieceTypes" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "type" TEXT UNIQUE NOT NULL);
CREATE TABLE "GamePieces" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "releaseKey" TEXT NOT NULL, "gamePieceTypeId" INTEGER NOT NULL, "userId" INTEGER NOT NULL, "value" TEXT NOT NULL);
CREATE TABLE "LibraryReleases" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "userId" INTEGER NOT NULL, "releaseKey" TEXT NOT NULL);
CREATE TABLE "ReleaseProperties" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "releaseKey" TEXT UNIQUE NOT NULL, "isDlc" INTEGER NOT NULL, "isVisibleInLibrary" INTEGER NOT NULL, "gameId" TEXT);
CREATE TABLE "UserReleaseProperties" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "releaseKey" TEXT NOT NULL, "userId" INTEGER NOT NULL, "isHidden" INTEGER NOT NULL);
CREATE TABLE "GameTimes" ("userId" INTEGER NOT NULL, "releaseKey" TEXT NOT NULL, "minutesInGame" INTEGER NOT NULL);
CREATE TABLE "LastPlayedDates" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "userId" INTEGER NOT NULL, "gameReleaseKey" TEXT NOT NULL, "lastPlayedDate" TEXT);
CREATE TABLE "InstalledBaseProducts" ("productId" INTEGER PRIMARY KEY NOT NULL, "installationPath" TEXT, "installationDate" TEXT);
CREATE TABLE "InstalledExternalProducts" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "platformId" INTEGER NOT NULL, "productId" TEXT NOT NULL, "installationDate" TEXT);
CREATE TABLE "Platforms" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "name" TEXT UNIQUE NOT NULL);
CREATE TABLE "ProductPurchaseDates" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "userId" INTEGER NOT NULL, "gameReleaseKey" TEXT NOT NULL, "purchaseDate" TEXT);
CREATE TABLE "Users" ("id" INTEGER PRIMARY KEY NOT NULL);

INSERT INTO Users VALUES (48000000000000001);
INSERT INTO GamePieceTypes (type) VALUES
  ('originalTitle'), ('title'), ('originalMeta'), ('meta'), ('originalImages'), ('images'),
  ('summary'), ('media'), ('myRating'), ('osCompatibility'), ('sortingTitle'), ('allGameReleases');
INSERT INTO Platforms (name) VALUES ('steam'), ('epic'), ('uplay'), ('gog');

INSERT INTO LibraryReleases (userId, releaseKey) VALUES
  (48000000000000001, 'gog_1207658924'),
  (48000000000000001, 'steam_620'),
  (48000000000000001, 'epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c'),
  (48000000000000001, 'gog_1495134320'),
  (48000000000000001, 'gog_1207664643');
INSERT INTO ReleaseProperties (releaseKey, isDlc, isVisibleInLibrary, gameId) VALUES
  ('gog_1207658924', 0, 1, '1207658924'),
  ('steam_620', 0, 1, NULL),
  ('epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c', 0, 1, NULL),
  ('gog_1495134320', 1, 1, '1495134320'),
  ('gog_1207664643', 0, 1, '1207664643');
INSERT INTO UserReleaseProperties (releaseKey, userId, isHidden) VALUES
  ('gog_1207664643', 48000000000000001, 1),
  ('steam_620', 48000000000000001, 0);

-- The store's pieces (original*), and the player's own title for one game.
INSERT INTO GamePieces (releaseKey, gamePieceTypeId, userId, value) VALUES
  ('gog_1207658924', 1, 48000000000000001, '{"title":"The Witcher: Enhanced Edition"}'),
  ('gog_1207658924', 2, 48000000000000001, '{"title":"The Witcher"}'),
  ('gog_1207658924', 3, 48000000000000001, '{"criticsScore":81,"developers":["CD PROJEKT RED"],"genres":["Role-playing"],"publishers":["CD PROJEKT RED"],"releaseDate":1221523200,"themes":["Fantasy"]}'),
  ('gog_1207658924', 5, 48000000000000001, '{"background":"https://images.gog.com/witcher_bg.jpg","squareIcon":"https://images.gog.com/witcher_sq.png","verticalCover":"https://images.gog.com/witcher_vc.jpg"}'),
  ('gog_1207658924', 7, 48000000000000001, '{"summary":"Hunt monsters for coin."}'),
  ('gog_1207658924', 8, 48000000000000001, '{"screenshots":["https://images.gog.com/witcher_1.jpg"],"videos":[]}'),
  ('gog_1207658924', 10, 48000000000000001, '{"supported":[{"name":"windows"}]}'),
  ('steam_620', 1, 48000000000000001, '{"title":"Portal 2"}'),
  ('steam_620', 3, 48000000000000001, '{"criticsScore":95,"developers":["Valve"],"genres":["Puzzle","Shooter"],"releaseDate":1303171200}'),
  ('steam_620', 5, 48000000000000001, '{"background":"https://images.gog.com/portal2_bg.jpg","squareIcon":null,"verticalCover":"https://images.gog.com/portal2_vc.jpg"}'),
  ('steam_620', 6, 48000000000000001, '{"background":"https://example.com/my_portal2_bg.jpg","squareIcon":null,"verticalCover":"https://example.com/my_portal2_vc.jpg"}'),
  ('epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c', 1, 48000000000000001, '{"title":"Hollow Hills"}'),
  ('epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c', 3, 48000000000000001, '{"developers":[],"genres":[],"releaseDate":null}'),
  ('gog_1495134320', 1, 48000000000000001, '{"title":"The Witcher: Bonus Soundtrack"}'),
  ('gog_1207664643', 1, 48000000000000001, '{"title":"A Hidden Game"}');

-- Two users' times for one game (the larger counts); none for Portal 2.
INSERT INTO GameTimes (userId, releaseKey, minutesInGame) VALUES
  (48000000000000001, 'gog_1207658924', 754),
  (48000000000000002, 'gog_1207658924', 12),
  (48000000000000001, 'epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c', 0);
INSERT INTO LastPlayedDates (userId, gameReleaseKey, lastPlayedDate) VALUES
  (48000000000000001, 'gog_1207658924', '2026-08-01 20:15:00'),
  (48000000000000001, 'steam_620', '2026-07-04T18:00:00Z'),
  (48000000000000001, 'epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c', NULL);
INSERT INTO ProductPurchaseDates (userId, gameReleaseKey, purchaseDate) VALUES
  (48000000000000001, 'gog_1207658924', '2019-06-01 10:00:00');

INSERT INTO InstalledBaseProducts VALUES (1207658924, 'C:\GOG Games\The Witcher', '2026-07-30 12:00:00');
INSERT INTO InstalledExternalProducts (platformId, productId, installationDate) VALUES (1, '620', '2026-07-01 09:00:00');
