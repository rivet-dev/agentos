/*
  Copyright (c) 1990-2001 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*****************************************************************************/
/*  Includes                                                                 */
/*****************************************************************************/

#include <string.h>
#include "unzvers.h"
#include <stdio.h>


#ifdef USE_SIOUX
#  include <sioux.h>
#  include <signal.h>
#  include <stdlib.h>
#  include <console.h>
#endif /* USE_SIOUX */


/*****************************************************************************/
/*  Global Vars                                                              */
/*****************************************************************************/

char fileList[256];


/*****************************************************************************/
/*  Prototypes                                                               */
/*****************************************************************************/

int UzpMain(int argc,char **argv);

char *GetUnZipLocalVersion(void);
char *GetUnZipInfoVersions(void);
int  macgetch(void);
void UserStop(void);



/*****************************************************************************/
/*  Functions                                                                */
/*****************************************************************************/

#ifndef MacStaticLib
#ifndef MACUNZIP_STANDALONE

/*
Program execution starts here with Metrowerks SIOUX-Console */
int main(int argc,char **argv)
{
    int return_code;

    SIOUXSettings.asktosaveonclose = FALSE;
    SIOUXSettings.showstatusline = TRUE;

    SIOUXSettings.columns = 100;
    SIOUXSettings.rows    = 40;

    argc = ccommand(&argv);

    return_code = UzpMain(argc,argv);

    printf("\n\n Finish  %d",return_code);

    return return_code;
}



int macgetch(void)
{
    WindowPtr whichWindow;
    EventRecord theEvent;
    char c;                     /* one-byte buffer for read() to use */

    do {
        SystemTask();
        if (!GetNextEvent(everyEvent, &theEvent))
            theEvent.what = nullEvent;
        else {
            switch (theEvent.what) {
            case keyDown:
                c = theEvent.message & charCodeMask;
                break;
            case mouseDown:
                if (FindWindow(theEvent.where, &whichWindow) ==
                    inSysWindow)
                    SystemClick(&theEvent, whichWindow);
                break;
            case updateEvt:
                break;
            }
        }
    } while (theEvent.what != keyDown);

    printf("*");
    fflush(stdout);

    return (int)c;
}


/* SIOUX needs no extra event handling */
void UserStop(void)
{
}

#endif  /*   #ifndef MACUNZIP_STANDALONE  */
#endif  /*   #ifndef MacStaticLib   */




char *GetUnZipLocalVersion(void)
{
static char UnZipVersionLocal[50];

memset(UnZipVersionLocal,0,sizeof(UnZipVersionLocal));

sprintf(UnZipVersionLocal, "[%s %s]", __DATE__, __TIME__);

return UnZipVersionLocal;
}




char *GetUnZipInfoVersions(void)
{
static char UnzipVersion[200];

memset(UnzipVersion,0,sizeof(UnzipVersion));

sprintf(UnzipVersion, "Unzip Module\n%d.%d%d%s of %s", UZ_MAJORVER,
        UZ_MINORVER, UZ_PATCHLEVEL, UZ_BETALEVEL, UZ_VERSION_DATE);

return UnzipVersion;
}
