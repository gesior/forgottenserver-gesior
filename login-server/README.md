# Serwer logowania

Osobny serwer protokołu logowania 10.97–10.98. Nasłuchuje na porcie 7173, trzyma konta i bany IP w pamięci i nie odpytuje bazy przy każdym logowaniu. Wyjątkiem jest spis postaci konta, gdy jego cache jest starszy niż `character_list_ttl_seconds`.

Hasło w kluczu sesji nadal jest jawne, bo serwer gry na porcie gry sam sprawdza je w bazie. Ten proces nie zastępuje wejścia do gry.

## Cache

Przy starcie, jeśli w tabeli `accounts` nie ma kolumny `updated_at`, proces wykonuje:

```sql
ALTER TABLE `accounts`
  ADD COLUMN `updated_at` TIMESTAMP NOT NULL
    DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  ADD INDEX `accounts_updated_at_idx` (`updated_at`, `id`);
```

Każdy `UPDATE` hasła, secretu albo `premium_ends_at` (także z strony konta) sam podbija ten znacznik. Poller co `account_poll_interval_seconds` dociąga tylko zmienione wiersze, więc nowe konto i nowe hasło działają po najbliższym pollu, nie w tej samej milisekundzie. Co `account_reconcile_interval_seconds` z pamięci znikają skasowane konta.

Bany IP i numer MOTD (`server_config.motd_num`) są odświeżane tym samym pollerem. Login ich nie czyta z bazy.

## Uruchomienie obok serwera gry

Z katalogu repozytorium. Oba kontenery łączą się z MySQL na hoście (`host.docker.internal`). Serwer gry słucha 7171/7172, login Rust na 7173 z `crypto_threads = 4`.

```powershell
docker compose up -d --build
```

Benchmark tworzy 50 tys. kont i 150 tys. postaci, potem przez 10 sekund mierzy logowania na istniejące i nieistniejące konto. Klient używa 12 wątków. Usługa `bench` nie startuje razem z serwerami.

```powershell
docker compose --profile bench run --rm bench
```

Profil `bench-hot` robi ten sam pomiar, ale logowania na istniejące konto kręcą się tylko po pierwszych 100 kontach z 50 tysięcy. Spis postaci tych kont zostaje w cache.

```powershell
docker compose --profile bench-hot run --rm bench-hot
```

Wynik to cztery liczby logowań na sekundę: Rust i OTS, osobno dla konta istniejącego (po rozgrzaniu cache spisu postaci) i dla nazwy, której nie ma w bazie.

## Budowanie samego obrazu logowania

Z katalogu repozytorium, kontekstem jest sam `login-server` (obraz testuje kod i zostawia binarkę):

```bash
docker build -f login-server/Dockerfile -t ots-login-server login-server
```

`cargo test --release` leci w trakcie budowania obrazu. Test migracji SQL uruchamia się tylko wtedy, gdy w środowisku budowania ustawiono `LOGIN_SERVER_DATABASE_URL`.

## Uruchomienie

`docker compose` używa gotowego [config.toml](config.toml) (4 wątki, baza `compilation` na `host.docker.internal`). Osobny `docker run` może wziąć `config.toml.example` i ten sam klucz RSA co serwer gry.

Lokalnie, z katalogu `login-server` (ścieżka `../key.pem`):

```bash
docker run --rm -p 7173:7173 ^
  -v "%CD%\config.toml:/config.toml" ^
  -v "%CD%\..\key.pem:/key.pem" ^
  ots-login-server /config.toml
```

W kontenerze ustaw `rsa_key_file = "/key.pem"`. Host MySQL z kontenera to zwykle `host.docker.internal`, nie `127.0.0.1`.

Klient logowania kierujesz na port 7173. Serwer gry może dalej słuchać 7171; żeby nie było dwóch loginów, ten port trzeba wygasić albo odciąć firewallem.

OpenSSL 3 jest linkowany dynamicznie. Obraz runtime zawiera `libssl3`.
