/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in zip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  win32/win32i64.c - UnZip 6

  64-bit filesize support for WIN32 Zip and UnZip.
  ---------------------------------------------------------------------------*/

#include "../zip.h"

/* --------------------------------------------------- */
/* Large File Support
 *
 * Initial functions by E. Gordon and R. Nausedat
 * 9/10/2003
 *
 * These implement 64-bit file support for Windows.  The
 * defines and headers are in win32/osdep.h.
 *
 * These moved from win32.c by Mike White to avoid conflicts
 * in WiZ of same name functions in UnZip and Zip libraries.
 * 9/25/04 EG
 */

#if defined(LARGE_FILE_SUPPORT) && !defined(__CYGWIN__)
# ifdef USE_STRM_INPUT

#  ifndef zftello
/* 64-bit buffered ftello
 *
 * Win32 does not provide a 64-bit buffered
 * ftell (in the published api anyway) so below provides
 * hopefully close version.
 * We have not gotten _telli64 to work with buffered
 * streams.  Below cheats by using fgetpos improperly and
 * may not work on other ports.
 */

zoff_t zftello(stream)
  FILE *stream;
{
  fpos_t fpos = 0;

  if (fgetpos(stream, &fpos) != 0) {
    return -1L;
  } else {
    return fpos;
  }
}
#  endif /* ndef zftello */


#  ifndef zfseeko
/* 64-bit buffered fseeko
 *
 * Win32 does not provide a 64-bit buffered
 * fseeko, so use _lseeki64 and fflush.  Note
 * that SEEK_CUR can lose track of location
 * if fflush is done between the last buffered
 * io and this call.
 */

int zfseeko(stream, offset, origin)
  FILE *stream;
  zoff_t offset;
  int origin;
{

  /* fseek() or its replacements are supposed to clear the eof status
     of the stream. fflush() and _lseeki64() do not touch the stream's
     eof flag, so we have to do it manually. */
#if ((defined(_MSC_VER) && (_MSC_VER >= 1200)) || \
     (defined(__MINGW32__) && defined(__MSVCRT_VERSION__)))
  /* For the MSC environment (VS 6 or higher), and for recent releases of
     the MinGW environment, we "know" the internals of the FILE structure.
     So, we can clear just the EOF bit of the status flag. */
  stream->_flag &= ~_IOEOF;
#else
  /* Unfortunately, there is no standard "cleareof()" function, so we have
     to use clearerr().  This has the unwanted side effect of clearing the
     ferror() state as well. */
  clearerr(stream);
#endif

  if (origin == SEEK_CUR) {
    /* instead of synching up lseek easier just to figure and
       use an absolute offset */
    offset += zftello(stream);
    origin = SEEK_SET;
  }
  fflush(stream);
  if (_lseeki64(fileno(stream), offset, origin) == (zoff_t)-1L) {
    return -1;
  } else {
    return 0;
  }
}
#  endif  /* ndef fseeko */
# endif /* USE_STRM_INPUT */
#endif  /* Win32 LARGE_FILE_SUPPORT */

#if 0
FILE* zfopen(filename, mode)
const char *filename;
const char *mode;
{
  FILE* fTemp;

  fTemp = fopen(filename, mode);
  if( fTemp == NULL )
    return NULL;

  /* sorry, could not make VC60 and its rtl work properly without setting the
   * file buffer to NULL. the problem seems to be _telli64 which seems to
   * return the max stream position, comments are welcome
   */
  setbuf(fTemp, NULL);

  return fTemp;
}
#endif
/* --------------------------------------------------- */
