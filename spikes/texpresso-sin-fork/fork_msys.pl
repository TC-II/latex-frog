# Costo del fork() emulado de Cygwin/MSYS2 con un proceso grande.
#
# Usa el perl de MSYS2 que trae Git for Windows ("C:\Program Files\Git\usr\bin\perl.exe"): su fork() es el de
# msys-2.0.dll, el mismo que tendría un TeXpresso compilado para Cygwin/MSYS2. Cygwin no tiene copy-on-write: crea un
# proceso nuevo y le copia .data, .bss, el heap y la pila con ReadProcessMemory/WriteProcessMemory, así que el costo
# crece con la memoria del proceso. Se mide con 0, 40 y 167 MB residentes (167 MB es el XeTeX de TeXpresso con el libro).
#
# Uso:  "C:\Program Files\Git\usr\bin\perl.exe" fork_msys.pl
use strict;
use warnings;
use Time::HiRes qw(time);

for my $mb (0, 40, 167) {
    my $buf = 'x' x ($mb * 1024 * 1024);
    my $n = 10;
    my ($sum, $max) = (0, 0);
    for (1 .. $n) {
        my $t0 = time;
        my $pid = fork();
        die "fork falló: $!" unless defined $pid;
        if ($pid == 0) {
            substr($buf, 0, 1) = 'y';
            require POSIX;
            POSIX::_exit(0);
        }
        waitpid($pid, 0);
        my $dt = (time - $t0) * 1000;
        $sum += $dt;
        $max = $dt if $dt > $max;
    }
    printf "fork de MSYS2 con %d MB: %.1f ms por fork+exit+wait (máx %.1f)\n", $mb, $sum / $n, $max;
}
