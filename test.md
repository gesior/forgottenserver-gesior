## RESULTS

final results:
```
warmup Rust existing: 8270.1/s, 50000 ok, 0 connect, 0 io, 0 other in 6.0s
Rust existing: 20561.1 logins/s (205621 ok, 0 connect, 0 io, 0 other, 10.0s)
Rust missing: 21184.8 logins/s (211860 ok, 0 connect, 0 io, 0 other, 10.0s)
warmup OTS existing: 1037.3/s, 50000 ok, 0 connect, 0 io, 0 other in 48.2s
OTS existing: 1021.5 logins/s (10224 ok, 0 connect, 0 io, 0 other, 10.0s)
OTS missing: 1379.9 logins/s (13809 ok, 0 connect, 0 io, 0 other, 10.0s)

scenario             logins/s         ok    connect         io      other
Rust existing         20561.1     205621          0          0          0
Rust missing          21184.8     211860          0          0          0
OTS existing           1021.5      10224          0          0          0
OTS missing            1379.9      13809          0          0          0
```

final results with 100 accounts (high cache hit rate):
```
warmup Rust hot: 5674.0/s, 100 ok, 0 connect, 0 io, 0 other in 0.0s
Rust hot: 19374.3 logins/s (193754 ok, 0 connect, 0 io, 0 other, 10.0s)
Rust missing: 20049.1 logins/s (200502 ok, 0 connect, 0 io, 0 other, 10.0s)
warmup OTS hot: 1014.7/s, 100 ok, 0 connect, 0 io, 0 other in 0.1s
OTS hot: 1008.0 logins/s (10088 ok, 0 connect, 0 io, 0 other, 10.0s)
OTS missing: 1378.4 logins/s (13795 ok, 0 connect, 0 io, 0 other, 10.0s)

scenario             logins/s         ok    connect         io      other
Rust hot              19374.3     193754          0          0          0
Rust missing          20049.1     200502          0          0          0
OTS hot                1008.0      10088          0          0          0
OTS missing            1378.4      13795          0          0          0
```

## PROMPT

Grok 4.7 Extra High

1st:
```
@protocollogin.cpp mam tutaj plik z protokolem logowania do konta do serwera OTS, teraz ten protokol jest czescia serwera OTS, ale chcialbym go uruchomic jako oddzielna aplikacje na porcie 7173
aktualnie kazde zapytanie do serwera laduje dane z bazy na zywo
chcialbym napisac oddzielny serwer, ktory co sekunde (do konfiguracji) bedzie wykrywal nowe konta w bazie, zmienione hasla do kont w bazie i cachowal na X sekund spis postaci z kont w bazie na ktore kto sie loguje, tak, zebym samo zapytanie z klienta gry o 'zalogowanie' nie wysylalo zapytan do bazy danych - z wyjatkiem odswiezenia cache 'spisu postaci', jesli jest zbyt stary
nowa aplikacja ma obslugiwac zapytania na wielu watkach, oprocz samego ladowania danych, duzym obciazeniem w obsludze zapytan jest odszyfrowanie ich z RSA i zaszyfrowanie odpowiedzi uzywajac XTEA
nie wiem w czym powinna byc napisana aplikacja pod maksymalna wydajnosc, musisz zdecydowac czy to ma byc rust, go czy jakis inny jezyk pozwalajacy na wydajne wspoldzielenie cache przez wiele watkow i obsluge zapytan sieciowych na wielu watkach
nowa aplikacja ma byc utworzona w podkatalogu 'login-server'
```

2nd:
```
dodaj docker-compose do odpalania tego serwera Rust, tak zebym mogl odpalic serwer OTS na 7171/7172 (to juz jest w glownym katalogu) i serwer logowania w Rust na 7173, oba beda mialy w konfiguracji polaczenie do bazy danych stojacej poza kontenerem
napisz tez skrypt do benchmarkowania, ktory utworzy 50k kont w bazie danych, doda do nich 150k postaci (po 3 na konto), a potem bedzie testowal ile zapytan na sekunde jest w stanie obsluzyc serwer Rust/OTS (C++) oraz ile logowan na 'nieistniejace konto' jest w stanie obsluzyc, do testow ogranicz ilosc watkow obslugujacych zapytania w Rust do 4 (tyle bedzie mial VPS, na produkcji), sam benchmark i serwer w Rust beda na tym samym komputerze podczas testow, ale on ma 24 rdzenie, wiec nie powinno byc problemow z wydajnoscia (niech skrypt benchmark uzywa 12 rdzeni, zeby nie moc obciazac wszystkich rdzeni i spowalniac znacznie serwera)
```

3rd:
```
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
```

4th:
```
chcialbym zrobic drugi profil testowy, kiedy zapytania 'hit' czesciej trafiaja w cache, czyli zeby test odwolywal sie w kolko do tych samych 100 kont z 50k kont utworzonych
```
