# Login server application design
_Exported on 11/1/2026 at 09:45:30 GMT+1 from Cursor (3.27.22)_

---

**User**

@src/protocollogin.cpp mam tutaj plik z protokolem logowania do konta do serwera OTS, teraz ten protokol jest czescia serwera OTS, ale chcialbym go uruchomic jako oddzielna aplikacje na porcie 7173
aktualnie kazde zapytanie do serwera laduje dane z bazy na zywo
chcialbym napisac oddzielny serwer, ktory co sekunde (do konfiguracji) bedzie wykrywal nowe konta w bazie, zmienione hasla do kont w bazie i cachowal na X sekund spis postaci z kont w bazie na ktore kto sie loguje, tak, zebym samo zapytanie z klienta gry o 'zalogowanie' nie wysylalo zapytan do bazy danych - z wyjatkiem odswiezenia cache 'spisu postaci', jesli jest zbyt stary
nowa aplikacja ma obslugiwac zapytania na wielu watkach, oprocz samego ladowania danych, duzym obciazeniem w obsludze zapytan jest odszyfrowanie ich z RSA i zaszyfrowanie odpowiedzi uzywajac XTEA
nie wiem w czym powinna byc napisana aplikacja pod maksymalna wydajnosc, musisz zdecydowac czy to ma byc rust, go czy jakis inny jezyk pozwalajacy na wydajne wspoldzielenie cache przez wiele watkow i obsluge zapytan sieciowych na wielu watkach
nowa aplikacja ma byc utworzona w podkatalogu 'login-server'

---

**Cursor**

Najpierw przejrzę obecny protokół logowania i to, jak serwer ładuje konta oraz postacie z bazy, żeby zaplanować oddzielną aplikację z cache.

Protokół z `protocollogin.cpp` jest krótki, ale kosztowny: dwa razy RSA-1024 bez paddingu i XTEA na odpowiedzi. Cache kont da się trzymać bez blokad na ścieżce logowania, a do bazy schodzić tylko po przedawniony spis postaci.

---

**User**

Implement the plan as specified, it is attached for your reference. Do NOT edit the plan file itself.

To-do's from the plan have already been created. Do not create them again. Mark them as in_progress as you work, starting with the first one. Don't stop until you have completed all the to-dos.

<timestamp>Saturday, Oct 31, 2026, 9:32 PM (UTC+1)</timestamp>
<system_notification>
The following task has finished. If you were already aware, ignore this notification and do not restate prior responses.

<task>
kind: shell
status: error
task_id: 746085
title: Check Rust toolchain and OpenSSL location
tool_call_id: call-1293527c-ef53-47de-80a6-1a20a4f59363-56
fc_3d2fc4a8-7a34-96a2-91c5-b7049bbfba9c_2
detail: exit_code=4294967295
output_path: C:\Users\JS\.cursor\projects\c-ots-forgottenserver-gesior\terminals\746085.txt
</task>
</system_notification>
<user_query>Briefly inform the user about the task result and perform any follow-up actions (if needed). If there's no follow-ups needed, don't explicitly say that.</user_query>

na tym komputrze nie ma rusta, cargo itp. do testow kompilacji i aplikacji uzywaj dockera

---

**Cursor**

Rust nie jest zainstalowany lokalnie, więc serwer logowania zbuduję w katalogu `login-server`, a kompilację i testy puszczę w Dockerze.

Serwer logowania jest w katalogu `login-server` i nasłuchuje na porcie **7173**. Obraz Dockera `ots-login-server` się buduje, a 17 testów protokołu i kryptografii przechodzi.

Logowanie czyta konta, hasła i bany IP z pamięci. Baza jest odpytywana przez poller (domyślnie co sekundę) oraz tylko wtedy, gdy spis postaci danego konta jest starszy niż 30 sekund. Nowe konto i nowe hasło zaczynają działać po najbliższym pollu, nie w tej samej milisekundzie. Przy pierwszym starcie proces sam doda kolumnę `accounts.updated_at`, jeśli jej nie ma. Ta sama kolumna jest już w `schema.sql`.

RSA i XTEA idą na stałej puli wątków (tyle, ile rdzeni). Tokio tylko przyjmuje połączenia. Klucz sesji nadal zawiera jawne hasło, bo wejście do gry na porcie 7172 samo sprawdza je w bazie.

Uruchomienie, po skopiowaniu `login-server/config.toml.example` do `config.toml` i ustawieniu w nim `rsa_key_file = "/key.pem"` oraz hosta MySQL (`host.docker.internal`, jeśli baza jest na tym komputerze):

```bash
docker build -f login-server/Dockerfile -t ots-login-server login-server
docker run --rm -p 7173:7173 -v "%CD%\login-server\config.toml:/config.toml" -v "%CD%\key.pem:/key.pem" ots-login-server /config.toml
```

Klient logowania kierujesz na port 7173. Serwer gry może dalej słuchać 7171. Żeby nie było dwóch loginów, ten port trzeba wygasić albo odciąć firewallem.

---

**User**

<timestamp>Saturday, Oct 31, 2026, 9:48 PM (UTC+1)</timestamp>
<system_notification>
The following task has finished. If you were already aware, ignore this notification and do not restate prior responses.

<task>
kind: shell
status: error
task_id: 746085
title: Check Rust toolchain and OpenSSL location
tool_call_id: call-1293527c-ef53-47de-80a6-1a20a4f59363-56
fc_3d2fc4a8-7a34-96a2-91c5-b7049bbfba9c_2
detail: exit_code=4294967295
output_path: C:\Users\JS\.cursor\projects\c-ots-forgottenserver-gesior\terminals\746085.txt
</task>
</system_notification>
<user_query>Briefly inform the user about the task result and perform any follow-up actions (if needed). If there's no follow-ups needed, don't explicitly say that.</user_query>

---

**Cursor**

Wyszukiwanie lokalnego Rusta i OpenSSL-a zostało przerwane. Na tym komputerze nie ma `cargo`, więc nie ma to znaczenia: serwer logowania został zbudowany i przetestowany w Dockerze, a obraz `ots-login-server` jest gotowy.

---

**User**

dodaj docker-compose do odpalania tego serwera Rust, tak zebym mogl odpalic serwer OTS na 7171/7172 (to juz jest w glownym katalogu) i serwer logowania w Rust na 7173, oba beda mialy w konfiguracji polaczenie do bazy danych stojacej poza kontenerem
napisz tez skrypt do benchmarkowania, ktory utworzy 50k kont w bazie danych, doda do nich 150k postaci (po 3 na konto), a potem bedzie testowal ile zapytan na sekunde jest w stanie obsluzyc serwer Rust/OTS (C++) oraz ile logowan na 'nieistniejace konto' jest w stanie obsluzyc, do testow ogranicz ilosc watkow obslugujacych zapytania w Rust do 4 (tyle bedzie mial VPS, na produkcji), sam benchmark i serwer w Rust beda na tym samym komputerze podczas testow, ale on ma 24 rdzenie, wiec nie powinno byc problemow z wydajnoscia (niech skrypt benchmark uzywa 12 rdzeni, zeby nie moc obciazac wszystkich rdzeni i spowalniac znacznie serwera)

---

**Cursor**

Sprawdzę, jak dziś jest odpalany serwer OTS w Dockerze i jak wygląda konfiguracja serwera logowania, żeby compose i benchmark dało się podpiąć do bazy poza kontenerem.

---

**User**

Implement the plan as specified, it is attached for your reference. Do NOT edit the plan file itself.

To-do's from the plan have already been created. Do not create them again. Mark them as in_progress as you work, starting with the first one. Don't stop until you have completed all the to-dos.

---

**Cursor**

Zaczynam od compose i konfiguracji z bazą na hoście, potem wydzielę bibliotekę i dopiszę benchmark.

Konfiguracja Compose idzie razem z wydzieleniem biblioteki, bo benchmark musi korzystać z tego samego RSA i formatu pakietu co serwer.

Serwer gry i serwer logowania startują razem z `docker-compose.yaml`. Gra zostaje na portach 7171 i 7172, login w Ruście na 7173, z czterema wątkami kryptografii. Oba łączą się z MySQL na hoście przez `host.docker.internal` — w `config.lua` host bazy jest już ustawiony tak samo, na użytkownika `root`, hasło `root` i bazę `compilation`.

Obraz logowania zbudował się w Dockerze, 18 testów przeszło. Benchmark jest osobną usługą i nie wstaje przy zwykłym starcie. Z katalogu repozytorium:

```powershell
docker compose up -d --build
docker compose --profile bench run --rm bench
```

Benchmark kasuje wcześniejsze konta `bench%`, wstawia 50 tysięcy kont z hasłem `benchmark` i 150 tysięcy postaci, potem na 12 wątkach mierzy 10 sekund logowań na istniejące konto i 10 sekund na nazwę, której nie ma w bazie. Osobno dla Rusta (`login:7173`) i OTS (`server:7171`). Przed pomiarem istniejących kont każde konto jest raz rozgrzane, żeby cache spisu postaci w Ruście był ciepły.

---

**User**

odpalilem benchmark, wyniki to:
seeded 50000 accounts and 150000 characters
encrypting login packets on 12 threads
login:7173 is accepting connections
server:7171 is accepting connections
warmup Rust existing: 50000 ok, 0 fail in 6.9s
Rust existing: 3328.3 logins/s (33288 ok, 35194 fail, 10.0s)
Rust missing: 0.0 logins/s (0 ok, 58498 fail, 10.0s)

warmup OTS existing: 50000 ok, 0 fail in 47.8s
OTS existing: 1048.7 logins/s (10494 ok, 0 fail, 10.0s)
OTS missing: 1393.1 logins/s (13944 ok, 0 fail, 10.0s)

scenario             logins/s         ok       fail
Rust existing          3328.3      33288      35194
Rust missing              0.0          0      58498
OTS existing           1048.7      10494          0
OTS missing            1393.1      13944          0

nie dosc, ze serwer w rust jest tylko 3 razy szybszy, to jeszcze raportuje polowe polaczen jako 'fail', co moze to powodowac? w serwerze w C++ ustawilem `allowOtcProxy = true` zeby nie bylo limitow polaczen z jednego IP, bo wczesniej po 5 probach logowania blokowalo pozostale

---

**Cursor**

Połowa logowań w Ruście pada dopiero po udanym rozgrzaniu, a brakujące konta padają w całości. Sprawdzę, czy serwer odrzuca te połączenia, czy klient źle je liczy.

Rozgrzanie Rusta przeszło w całości (około 7200/s), a błędy zaczynają się dopiero potem i są zbyt szybkie na timeout RSA. To wygląda na wyczerpanie portów po dziesiątkach tysięcy połączeń, nie na odrzucenie logowania przez serwer. Poprawię pomiar, żeby to rozdzielić i nie zatykać portów.

Te `fail` nie oznaczają, że serwer Rust odrzucił logowanie. Benchmark wrzucał do jednego licznika wszystko, co nie było oczekiwaną odpowiedzią: błąd połączenia, zerwany odczyt i timeout.

Rozgrzanie to pokazuje. 50 000 istniejących kont przeszło bez żadnego błędu w 6,9 s, czyli około **7250 logowań/s**. To jest realny wynik Rusta dla istniejącego konta, około siedem razy więcej niż 1048/s u OTS, a nie trzy. Pomiar tuż potem jest już zepsuty: każde logowanie to nowe połączenie TCP, które po zamknięciu zostaje w `TIME_WAIT` i trzyma port źródłowy przez około minutę. Po tych 50 tysiącach zakres portów (albo tabela połączeń Dockera) się kończy. Dalsze próby padają od razu, dlatego nieistniejące konta mają 0% sukcesu i aż 58 tysięcy błędów w 10 sekund. Przy 12 wątkach i dwusekundowym timeoutcie tyle błędów nie da się wyprodukować czekaniem na odpowiedź.

Limit „5 prób z jednego IP” jest tylko w serwerze C++, w `Ban::acceptConnection`. `allowOtcProxy = true` go wyłącza. Serwer Rust takiego limitu nie ma.

Benchmark zamyka teraz połączenie przez RST, żeby nie zostawiać `TIME_WAIT`, i rozdziela błędy na `connect` (brak portu), `io` (zerwany odczyt albo timeout) i `other` (przyszła inna poprawna odpowiedź). Obraz jest przebudowany. Powtórka:

```powershell
docker compose --profile bench run --rm --build bench
```

---

**User**

chcialbym zrobic drugi profil testowy, kiedy zapytania 'hit' czesciej trafiaja w cache, czyli zeby test odwolywal sie w kolko do tych samych 100 kont z 50k kont utworzonych

---

**Cursor**

Drugi profil ma kręcić się tylko po 100 kontach z tych 50 tysięcy, żeby spis postaci był już w cache. Sprawdzę, jak benchmark teraz wybiera konta.

Drugi profil zostawi w bazie te same 50 tysięcy kont, ale pomiar istniejących logowań będzie kręcił się tylko po pierwszych 100, żeby spis postaci zostawał w cache.

Profil `bench-hot` nadal tworzy 50 tysięcy kont, ale logowania na istniejące konto kręcą się tylko po pierwszych 100 (`bench000000`–`bench000099`). Po rozgrzaniu ich spis postaci zostaje w cache i kolejne zapytania już go nie odświeżają. Test nieistniejących kont jest bez zmian.

```powershell
docker compose --profile bench-hot run --rm --build bench-hot
```

W tabeli te scenariusze nazywają się `Rust hot` i `OTS hot`. Pełny obieg po wszystkich kontach zostaje pod dotychczasowym profilem `bench`.

