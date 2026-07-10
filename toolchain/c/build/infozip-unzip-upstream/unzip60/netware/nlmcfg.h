/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#ifdef NLM
#  include <sys/types.h>
#  include <sys/stat.h>
#  include <fcntl.h>
#  include <unistd.h>
#  include <utime.h>
#  include <nwfileio.h>
#  define DIRENT
#  include <time.h>
#  ifndef DATE_FORMAT
#    define DATE_FORMAT DF_MDY
#  endif
#  define lenEOL          2
#  define PutNativeEOL  {*q++ = native(CR); *q++ = native(LF);}
#  define USE_FWRITE    /* write() fails to support textmode output */
#  if (!defined(NOTIMESTAMP) && !defined(TIMESTAMP))
#    define TIMESTAMP
#  endif
#  define MAIN main
#  define DECLARE_TIMEZONE
#  define SCREENWIDTH 80
#  define SCREENSIZE(scrrows, scrcols)  screensize(scrrows, scrcols)
   void InitUnZipConsole OF((void));
   int screensize        OF((int *tt_rows, int *tt_cols));
#endif /* NLM */
