Limiter les fsync : ne fais pas de sync_data sur chaque put/delete; groupe les écritures (buffer + flush périodique ou sync_interval configurable).
Batches WAL : sérialise plusieurs entrées dans un seul bloc pour réduire les appels write.
Cache en écriture : garde les valeurs chaudes/mises à jour en mémoire (write-back + flush).
Compaction asynchrone : lance la réécriture du log dans un thread dédié pour ne pas bloquer les opérations.
I/O buffered : utilise BufWriter/BufReader ou mmap pour le WAL.
Compression optionnelle : zstd/snappy peut réduire les octets écrits, donc moins d’E/S.
Benchmarks plus longs : mesure avec cargo bench après warm-up pour voir l’impact réel des changements