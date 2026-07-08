/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  unzipstb.c

  Simple stub function for UnZip DLL (or shared library, whatever); does
  exactly the same thing as normal UnZip, except for additional printf()s
  of various version numbers, solely as a demonstration of what can/should
  be checked when using the DLL.  (If major version numbers ever differ,
  assume program is incompatible with DLL--especially if DLL version is
  older.  This is not likely to be a problem with *this* simple program,
  but most user programs will be much more complex.)

  ---------------------------------------------------------------------------*/

#include <stdio.h>
#include "unzip.h"
#if defined(MODERN) && !defined(NO_STDDEF_H)
# include <stddef.h>
#endif
#include "unzvers.h"

int main(int argc, char *argv[])
{
    static ZCONST UzpVer *pVersion;     /* no pervert jokes, please... */

    pVersion = UzpVersion();

    printf("UnZip stub:  checking version numbers (DLL is dated %s)\n",
      pVersion->date);
    printf("   UnZip versions:    expecting %u.%u%u, using %u.%u%u%s\n",
      UZ_MAJORVER, UZ_MINORVER, UZ_PATCHLEVEL, pVersion->unzip.major,
      pVersion->unzip.minor, pVersion->unzip.patchlevel, pVersion->betalevel);
    printf("   ZipInfo versions:  expecting %u.%u%u, using %u.%u%u\n",
      ZI_MAJORVER, ZI_MINORVER, UZ_PATCHLEVEL, pVersion->zipinfo.major,
      pVersion->zipinfo.minor, pVersion->zipinfo.patchlevel);

/*
    D2_M*VER and os2dll.* are obsolete, though retained for compatibility:

    printf("   OS2 DLL versions:  expecting %u.%u%u, using %u.%u%u\n",
      D2_MAJORVER, D2_MINORVER, D2_PATCHLEVEL, pVersion->os2dll.major,
      pVersion->os2dll.minor, pVersion->os2dll.patchlevel);
 */

    if (pVersion->flag & 2)
        printf("   using zlib version %s\n", pVersion->zlib_version);

    /* This example code only uses the dll calls UzpVersion() and
     * UzpMain().  The APIs for these two calls have maintained backward
     * compatibility since at least the UnZip release 5.3 !
     */
#   define UZDLL_MINVERS_MAJOR          5
#   define UZDLL_MINVERS_MINOR          3
#   define UZDLL_MINVERS_PATCHLEVEL     0
    /* This UnZip DLL stub requires a DLL version of at least: */
    if ( (pVersion->unzip.major < UZDLL_MINVERS_MAJOR) ||
         ((pVersion->unzip.major == UZDLL_MINVERS_MAJOR) &&
          ((pVersion->unzip.minor < UZDLL_MINVERS_MINOR) ||
           ((pVersion->unzip.minor == UZDLL_MINVERS_MINOR) &&
            (pVersion->unzip.patchlevel < UZDLL_MINVERS_PATCHLEVEL)
           )
          )
         ) )
    {
        printf("  aborting because of too old UnZip DLL version!\n");
        return -1;
    }

    /* In case the offsetof() macro is not supported by some C compiler
       environment, it might be replaced by something like:
         ((extent)(void *)&(((UzpVer *)0)->dllapimin))
     */
    if (pVersion->structlen >=
#if defined(MODERN) && !defined(NO_STDDEF_H)
        ( offsetof(UzpVer, dllapimin)
#else
          ((unsigned)&(((UzpVer *)0)->dllapimin))
#endif
         + sizeof(_version_type) ))
    {
#ifdef OS2DLL
#       define UZ_API_COMP_MAJOR        UZ_OS2API_COMP_MAJOR
#       define UZ_API_COMP_MINOR        UZ_OS2API_COMP_MINOR
#       define UZ_API_COMP_REVIS        UZ_OS2API_COMP_REVIS
#else /* !OS2DLL */
#ifdef WINDLL
#       define UZ_API_COMP_MAJOR        UZ_WINAPI_COMP_MAJOR
#       define UZ_API_COMP_MINOR        UZ_WINAPI_COMP_MINOR
#       define UZ_API_COMP_REVIS        UZ_WINAPI_COMP_REVIS
#else /* !WINDLL */
#       define UZ_API_COMP_MAJOR        UZ_GENAPI_COMP_MAJOR
#       define UZ_API_COMP_MINOR        UZ_GENAPI_COMP_MINOR
#       define UZ_API_COMP_REVIS        UZ_GENAPI_COMP_REVIS
#endif /* ?WINDLL */
#endif /* ?OS2DLL */
        printf(
          "   UnZip API version: can handle <= %u.%u%u, DLL supplies %u.%u%u\n",
          UZ_API_COMP_MAJOR, UZ_API_COMP_MINOR, UZ_API_COMP_REVIS,
          pVersion->dllapimin.major, pVersion->dllapimin.minor,
          pVersion->dllapimin.patchlevel);
        if ( (pVersion->dllapimin.major > UZ_API_COMP_MAJOR) ||
             ((pVersion->dllapimin.major == UZ_API_COMP_MAJOR) &&
              ((pVersion->dllapimin.minor > UZ_API_COMP_MINOR) ||
               ((pVersion->dllapimin.minor == UZ_API_COMP_MINOR) &&
                (pVersion->dllapimin.patchlevel > UZ_API_COMP_REVIS)
               )
              )
             ) )
        {
            printf("  aborting because of unsupported dll api version!\n");
            return -1;
        }
    }
    printf("\n");

    /* call the actual UnZip routine (string-arguments version) */
    return UzpMain(argc, argv);
}
