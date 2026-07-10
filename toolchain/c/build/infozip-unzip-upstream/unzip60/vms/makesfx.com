$!
$!  MAKESFX.COM:  command-procedure to create self-extracting ZIP archives
$!                usage:  @MAKESFX foo    (foo.zip -> foo.exe)
$!
$!  Change history:
$!
$!  Date      Who   What
$!  --------  ----  -----------------------------------------------------------
$!  19940804  MPJZ  Created
$!  19940810  GRR   Removed superflous creation of name symbol
$!  20000113  MPJZ  Better symbol check, fixed bug in zip "-A" check
$!
$!  MPJZ: Martin P.J. Zinser
$!
$!  For this to work a symbol unzipsfx has to be defined which contains the
$!  location of the unzip stub (e.g., unzipsfx:== device:[dir]unzipsfx.exe)
$!
$!  The zipfile given in p1 will be concatenated with unzipsfx and given a
$!  filename extension of .exe.  The default file extension for p1 is .zip
$!
$!  Use at your own risk, there is no guarantee here.  If it doesn't work,
$!  blame me (zinser@decus.de), not the people from Info-ZIP.
$!
$!-----------------------------------------------------------------------------
$!
$! First check stub related stuff
$!
$ if (f$type(unzipsfx).nes."STRING")
$ then
$   type sys$input
You need to define the symbol "unzipsfx" to point to the location of the
unzipsfx stub before invoking this procedure.
Exiting now...
$   exit 2
$ endif
$ usfx = f$parse(unzipsfx) - ";"
$ if (f$search(usfx).eqs."")
$ then
$   write sys$output "The unzipsfx stub can not be found on the location"
$   write sys$output "pointed to by the unzipsfx symbol -- ''usfx'"
$   write sys$output "Exiting now"
$   exit 2
$ endif
$!
$! Now check the input file
$!
$ if (p1.eqs."")
$ then
$   type sys$input
Required parameter input-file missing
Exiting now...
$   exit 2
$ endif
$ inf = p1
$ file = f$parse(inf,,,"DEVICE") + f$parse(inf,,,"DIRECTORY") + -
         f$parse(inf,,,"NAME")
$ finf = file + f$parse(inf,".ZIP",,"TYPE") + f$parse(inf,,,"VERSION")
$ if (f$search(finf).eqs."")
$ then
$   write sys$output "Input file ''finf' does not exist"
$   exit 2
$ endif
$!
$! Finally create the self-extracting archive
$!
$ copy 'usfx','finf' 'file'.exe
$!
$! Zip "-A" will make the resulting archive compatible with other
$! unzip programs, but is not essential for running the exe.
$!
$ if (f$type(zip).eqs."STRING") then zip "-A" 'file'.exe
$ exit
