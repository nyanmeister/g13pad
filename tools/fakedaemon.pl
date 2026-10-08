#!/usr/bin/perl
# A stand-in for g13d's pipe reader: holds the FIFO O_RDWR like the daemon, and once per
# 100 ms (its USB read timeout) takes whatever the pipe holds in ONE read, like
# ReadCommandsFromPipe (1 MB buffer). Logs each read: time, size, and either "FRAME <md5>"
# for exactly 960 bytes or the text. Usage: fakedaemon.pl FIFO LOG
use strict; use warnings; use Fcntl; use Time::HiRes qw(time sleep); use Digest::MD5 qw(md5_hex);
my ($fifo, $log, $raw) = @ARGV;
sysopen(my $f, $fifo, O_RDWR) or die "$fifo: $!";
open(my $l, '>>', $log) or die "$log: $!";
select($l); $| = 1;
my $t0 = time;
while (1) {
    my $rin = ''; vec($rin, fileno($f), 1) = 1;
    if (select($rin, undef, undef, 0) > 0) {
        my $n = sysread($f, my $buf, 1024 * 1024);
        my $t = sprintf('%8.3f', time - $t0);
        if ($raw) { print $l $buf; }
        elsif ($n == 960) { printf $l "%s FRAME %s\n", $t, md5_hex($buf); }
        else { (my $s = $buf) =~ s/\n/\\n/g; printf $l "%s %d bytes: %s\n", $t, $n, substr($s, 0, 100); }
    }
    sleep(0.1);
}
