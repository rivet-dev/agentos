This is the README file for the 20 April 2009 public release of the
Info-ZIP group's portable UnZip zipfile-extraction program (and related
utilities).

unzip60.zip       portable UnZip, version 6.0, source code distribution
unzip60.tar.Z     same as above, but compress'd tar format
unzip60.tar.gz    same as above, but gzip'd tar format

__________________________________________________________________________

BEFORE YOU ASK:  UnZip, its companion utility Zip, and related utilities
and support files can be found in many places; read the file "WHERE" for
further details.  To contact the authors with suggestions, bug reports,
or fixes, continue reading this file (README) and, if this is part of a
source distribution, the file "ZipPorts" in the proginfo directory.  Also
in source distributions:  read "BUGS" for a list of known bugs, non-bugs
and possible future bugs; INSTALL for instructions on how to build UnZip;
and "Contents" for a commented listing of all the distributed files.
__________________________________________________________________________


GENERAL INFO
------------
UnZip is an extraction utility for archives compressed in .zip format (also
called "zipfiles").  Although highly compatible both with PKWARE's PKZIP
and PKUNZIP utilities for MS-DOS and with Info-ZIP's own Zip program, our
primary objectives have been portability and non-MSDOS functionality.

This version of UnZip has been ported to a stupendous array of hardware--
from micros to supercomputers--and operating systems:  Unix (many flavors),
VMS, OS/2 (including DLL version), Windows NT and Windows 95 (including DLL
version), Windows CE (GUI version), Windows 3.x (including DLL version),
MS-DOS, AmigaDOS, Atari TOS, Acorn RISC OS, BeOS, Macintosh (GUI version),
SMS/QDOS, MVS, VM/CMS, FlexOS, Tandem NSK, Human68k (mostly), AOS/VS (partly)
and TOPS-20 (partly).  UnZip features not found in PKUNZIP include source
code; default extraction of directory trees (with a switch to defeat this,
rather than the reverse); system-specific extended file attributes; and, of
course, the ability to run under most of your favorite operating systems.
Plus, it's free. :-)

For source distributions, see the main Contents file for a list of what's
included, and read INSTALL for instructions on compiling (including OS-
specific comments).  The individual operating systems' Contents files (for
example, vms/Contents) may list important compilation info in addition to
explaining what files are what, so be sure to read them.  Some of the ports
have their own, special README files, so be sure to look for those, too.

See unzip.1 or unzip.txt for usage (or the corresponding UnZipSFX, ZipInfo,
fUnZip and ZipGrep docs).  For VMS, unzip_def.rnh or unzip_cli.help may be
compiled into unzip.hlp and installed as a normal VMS help entry; see
vms/descrip.mms.


CHANGES AND NEW FEATURES
------------------------
UnZip 6.0 finally supports nowadays "large" files of sizes > 2 GiB!
This is the first release containing support for the PKWARE Zip64
enhancements.
Major changes are:
   - Support PKWARE ZIP64 extensions, allowing Zip archives and Zip archive
     entries larger than 4 GiBytes and more than 65536 entries within a single
     Zip archive. This support is currently only available for Unix,
     OpenVMS and Win32/Win64.
   - Support for bzip2 compression method.
   - Support for UTF-8 encoded entry names, both through PKWARE's "General
     Purpose Flags Bit 11" indicator and Info-ZIP's new "up" unicode path
     extra field.  (Currently, on Windows the UTF-8 handling is limited to
     the character subset contained in the configured non-unicode "system
     code page".)
   - Added "wrong implementation used" warning to error messages of the MSDOS
     port when used under Win32, in an attempt to reduce false bug reports.
   - Fixed "Time of Creation/Time of Use" vulnerability when setting attributes
     of extracted files, for Unix and Unix-like ports.
   - Fixed memory leak when processing invalid deflated data.
   - Fixed long-standing bug in unshrink (partial_clear), added boundary checks
     against invalid compressed data.
   - On Unix, keep inherited SGID attribute bit for extracted directories
     unless restoration of owner/group id or SUID/SGID/Tacky attributes was
     requested.
   - On Unix, allow extracted filenames to contain embedded control characters
     when explicitly requested by specifying the new command line option "-^".
   - On Unix, support restoration of symbolic link attributes.
   - On Unix, support restoration of 32-bit UID/GID data using the new "ux"
     IZUNIX3 extra field introduced with Zip 3.0.
   - Support for ODS5 extended filename syntax on new OpenVMS systems.
   - Support symbolic links zipped up on VMS.
   - On VMS (only 8.x or better), support symbolic link creation.
   - On VMS, support option to create converted text files in Stream_LF format.
   - New -D option to suppress restoration of timestamps for extracted
     directory entries (on those ports that support setting of directory
     timestamps).  By specifying "-DD", this new option also allows to suppress
     timestamp restoration for ALL extracted files on all UnZip ports which
     support restoration of timestamps.
     On VMS, the default behaviour is now to skip restoration of directory
     timestamps; here, "--D" restores ALL timestamps, "-D" restores none.
   - On OS/2, Win32, and Unix, the (previously optional) feature UNIXBACKUP
     to allow saving backup copies of overwritten files on extraction is now
     enabled by default.

For the UnZip 6.0 release, we want to give special credit to Myles Bennet,
who started the job of supporting ZIP64 extensions and Large-File (> 2GiB)
and provided a first (alpha-state) port.

The 5.52 maintenance release fixes a few minor problems found in the 5.51
release, closes some more security holes, adds a new AtheOS port, and
contains a Win32 extra-field code cleanup that was not finished earlier.
The most important changes are:

   - (re)enabled unshrinking support by default, the LZW patents have expired
   - fixed an extraction size bug for encrypted stored entries (12 excess bytes
     were written with 5.51)
   - fixed false "uncompressed size mismatch" messages when extracting
     encrypted archive entries
   - do not restore SUID/SGID/Tacky attribute bits on Unix (BeOS, AtheOS)
     unless explicitely requested by new "-K" command line qualifier
   - optional support for "-W" qualifier to modify the pattern matching syntax
     (with -W: "*" stops at directory delimiter, "**" matches unlimited)
   - prevent buffer overflow caused by bogus extra-long Zipfile specification
   - performance enhancements for VMS port
   - fixed windll interface handling of its extraction mode qualifiers
     nfflag, ExtractOnlyNewer, noflag, PromptToOverwrite; added detailed
     explanation of their meanings and interactions to the windll documentation

The 5.51 maintenance release adds a command-line CE port, intended for
batch processing. With the integration of this port, the pUnZip port
has been revised and "revitalized".
The most important changes for the general public are a number of
bug fixes, mostly related to security issues:

   - repair a serious bug in the textmode output conversion code for the 16-bit
     ports (16-bit MSDOS, OS/2 1.x, some variants of AMIGA, possibly others)
     which was introduced by the Deflate64 support of release 5.5
   - fix a long standing bug in the the inflate decompression method that
     prevented correct extraction in some rare cases
   - fixed holes in parent dir traversal security code (e.g.: ".^C." slipped
     through the previous version of the check code)
   - fixed security hole: check naming consistency in local and central header
   - fixed security hole: prevent extracted symlinks from redirecting file
     extraction paths

The main addition in the 5.5 release is support for PKWARE's new Deflate64(tm)
algorithm, which appeared first in PKZIP 4.0 (published November 2000).
As usual, some other bugfixes and clean-ups have been integrated:

   - support for Deflate64 (Zip compression method #9)
   - support for extracting VMS variable length record text files on
     any system
   - optional "cheap autorun" feature for the SFX stub
   - security fixes:
     * strip leading slash from stored pathspecs,
     * remove "../" parent dir path components from extracted file names
   - new option "-:" to allow verbatim extraction of file names containing
     "../" parent dir path specs
   - fixed file handle leak for the DLL code
   - repaired OS2 & WinNT ACL extraction which was broken in 5.42

The 5.42 maintenance release fixes more bugs and cleans up the redistribution
conditions:

   - removal of unreduce.c and amiga/timelib.c code to get rid of the last
     distribution restrictions beyond the BSD-like Info-ZIP LICENSE
   - new generic timelib replacement (currently used by AMIGA port)
   - more reasonable mapping rules of UNIX "leading-dot" filenames to the
     DOS 8.3 name convention
   - repaired screensize detection in MORE paging code
     (was broken for DOS/OS2/WIN32 in 5.41)

The 5.41 maintenance release adds another new port and fixes some bugs.

   - new BSD-like LICENSE
   - new Novell Netware NLM port
   - supports extraction of archives with more than 64k entries
   - attribute handling of VMS port was broken in UnZip 5.4
   - decryption support integrated in the main source distribution

The 5.4 release adds new ports, again. Other important items are changes
to the listing format, new supplemental features and several bug fixes
(especially concerning time-stamp handling...):

   - new IBM OS/390 port, a UNIX derivate (POSIX with EBCDIC charset)
   - complete revision of the MacOS port
   - changed listing formats to enlarge the file size fields for more digits
   - added capability to restore directory attributes on MSDOS, OS/2, WIN32
   - enabled support of symbolic links on BeOS
   - Unix: optional Acorn filetype support, useful for volumes exported via NFS
   - several changes/additions to the DLL API
   - GUI SFX stub for Win16 (Windows 3.1) and Win32 (Windows 9x, Windows NT)
   - new free GCC compiler environments supported on WIN32
   - many time-zone handling bug fixes for WIN32, AMIGA, ...

The 5.32 release adds two new ports and a fix for at least one relatively
serious bug:

   - new FlexOS port
   - new Tandem NSK port
   - new Visual BASIC support (compatibility with the Windows DLLs)
   - new -T option (set zipfile timestamp) for virtually all ports
   - fix for timestamps beyond 2038 (e.g., 2097; crashed under DOS/Win95/NT)
   - fix for undetected "dangling" symbolic links (i.e., no pointee)
   - fix for VMS indexed-file extraction problem (stored with Zip 2.0 or 2.1)
   - further performance optimizations

The 5.31 release included nothing but small bug-fixes and typo corrections,
with the exception of some minor performance tweaks.

The 5.3 release added still more ports and more cross-platform portability
features:

   - new BeOS port
   - new SMS/QDOS port
   - new Windows CE graphical port
   - VM/CMS port fully updated and tested
   - MVS port fully updated and tested
   - updated Windows DLL port, with WiZ GUI spun off to a separate package
   - full Universal Time (UTC or GMT) support for trans-timezone consistency
   - cross-platform support for 8-bit characters (ISO Latin-1, OEM code pages)
   - support for NT security descriptors (ACLs)
   - support for overwriting OS/2 directory EAs if -o option given
   - updated Solaris/SVR4 package facility

What is (still!) not added is multi-part archive support (a.k.a. "diskette
spanning", though we really mean archive splitting and not the old diskette
spanning) and a unified and more powerful DLL interface.  These are the two
highest priorities for the 6.x releases.  Work on the former is almost
certain to have commenced by the time you read this.  This time we mean it!
You betcha. :-)

Although the DLLs are still basically a mess, the Windows DLLs (16- and 32-
bit) now have some documentation and a small example application.  Note that
they should now be compatible with C/C++, Visual BASIC and Delphi.  Weirder
languages (FoxBase, etc.) are probably Right Out.


INTERNET RESOURCES
------------------

Info-ZIP's web site is at http://www.info-zip.org/pub/infozip/
and contains the most up-to-date information about coming releases,
links to binaries, and common problems.
(See http://www.info-zip.org/pub/infozip/FAQ.html for the latter.)
Files may also be retrieved via ftp://ftp.info-zip.org/pub/infozip/ .
Thanks to LEO (Munich, Germany) for previously hosting our primary site.


DISTRIBUTION
------------
If you have a question regarding redistribution of Info-ZIP software, either
as is, as packaging for a commercial product, or as an integral part of a
commercial product, please read the Frequently Asked Questions (FAQ) section
of the included COPYING file.  All Info-ZIP releases are now covered by
the Info-ZIP license.  See the file LICENSE.  The most current license
should be available at http://www.info-zip.org/license.html and
ftp://ftp.info-zip.org/pub/infozip/license.html.

Insofar as C compilers are rare on some platforms and the authors only have
direct access to a subset of the supported systems, others may wish to pro-
vide ready-to-run executables for new systems.  In general there is no prob-
lem with this; we require only that such distributions include this README
file, the WHERE file, the LICENSE file (contains copyright/redistribution
information), and the appropriate documentation files (unzip.txt and/or
unzip.1 for UnZip, etc.).  If the local system provides a way to make self-
extracting archives in which both the executables and text files can be
stored together, that's best (in particular, use UnZipSFX if at all possible,
even if it's a few kilobytes bigger than the alternatives); otherwise we
suggest a bare UnZip executable and a separate zipfile containing the re-
maining text and binary files.  If another archiving method is in common
use on the target system (for example, Zoo or LHa), that may also be used.


BUGS AND NEW PORTS:  CONTACTING INFO-ZIP
----------------------------------------
All bug reports and patches (context diffs only, please!) should be
submitted either through the new Info-ZIP Discussion Forum at
http://www.info-zip.org/board/board.pl or through the Info-ZIP SourceForge
site at http://sourceforge.net/projects/infozip/.  The forum allows file
attachments while SourceForge provides a place to post patches.  The old
Zip-Bugs@lists.wku.edu e-mail address for the Info-ZIP authors was
discontinued after heavy continuous spam, as was the QuickTopic discussion
forum.  The above methods are public, but we also can be reached directly
using the web reply page at http://www.info-zip.org/zip-bug.html.  If you
need to send us files privately, contact us first for instructions.

"Dumb questions" that aren't adequately answered in the documentation
should also be directed to Zip-Bugs rather than to a global forum such
as Usenet.  (Kindly make certain that your question *isn't* answered by
the documentation, however--a great deal of effort has gone into making
it clear and complete.)

Suggestions for new features can be discussed on the new Discussion Forum.
A new mailing list for Info-ZIP beta testers and interested parties may
be created someday, but for now any issues found in the betas should use
the forum.  We make no promises to act on all suggestions or even all
patches, but if it is something that is manifestly useful, sending the
required patches to Zip-Bugs directly (as per the instructions in the
ZipPorts file) is likely to produce a quicker response than asking us to
do it--the authors are always ridiculously short on time.  (Please do
NOT send patches or encoded zipfiles to the Info-ZIP list.  Please DO
read the ZipPorts file before sending any large patch.  It would be
difficult to over-emphasize this point...)

If you are considering a port, not only should you read the ZipPorts file,
but also please check in with Zip-Bugs BEFORE getting started, since the
code is constantly being updated behind the scenes.  (For example, VxWorks,
VMOS and Netware ports were once claimed to be under construction, although
we have yet to see any up-to-date patches.)  We will arrange to send you the
latest sources.  The alternative is the possibility that your hard work will
be tucked away in a subdirectory and mostly ignored, or completely ignored
if someone else has already done the port (and you'd be surprised how often
this has happened).


BETA TESTING:  JOINING INFO-ZIP
-------------------------------
If you'd like to keep up to date with our UnZip (and companion Zip utility)
development, join the ranks of beta testers, add your own thoughts and
contributions, or simply lurk, you may join one of our mailing lists.
There is an announcements-only list (Info-ZIP-announce) and a general
discussion/testing list (Info-ZIP). You must be a subscriber to post, and
you can subscribe via the links on our Frequently Asked Questions page:

        http://www.info-zip.org/pub/infozip/FAQ.html#lists

(Please note that as of late May 2004, the lists are unavailable pending
a move to a new site; we hope to have them restored shortly.  In the
interim ...)  Feel free to use our bug-reporting web page for bug reports
and to ask questions not answered on the FAQ page above:

        http://www.info-zip.org/zip-bug.html

For now the best option is to monitor and contribute to the various threads
on the new discussion forum site at:

      http://www.info-zip.org/board/board.pl

The second best way to contribute is through the various features at
SourceForge, such as the bug posting areas.

There is also a closed mailing list for internal discussions of our core
development team. This list is now kept secret to prevent us from being
flooded with spam messages.


-- Greg Roelofs (sometimes known as Cave Newt), principal UnZip developer
   guy, with inspiration from David Kirschbaum, was Author of this text.

-- Christian Spieler (shorthand: SPC), current UnZip maintenance coordinator,
   applied the most recent changes, with Ed Gordon providing a few additions.
