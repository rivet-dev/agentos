/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  macscreen.c
  This file is only linked into the standalone version (not SIOUX) of unzip.
  Macintosh-GUI routines.

  ---------------------------------------------------------------------------*/


/*****************************************************************************/
/*  Includes                                                                 */
/*****************************************************************************/

#include <QuickDraw.h>

#include <stdio.h>
#include <stdarg.h>
#include <string.h>


/*****************************************************************************/
/*  Macros, typedefs                                                         */
/*****************************************************************************/

#define bufferSize      4096

#define screenWindow    128

#define pauseOption     0x0001
#define scrollOption    0x0002


/*****************************************************************************/
/*  Module level Vars                                                        */
/*****************************************************************************/

static Rect scrollRect, pauseRect;
static WindowPtr theWindow;
static RgnHandle scrollRgn;

static short fontHeight, fontWidth, screenHeight, screenWidth;
static short currentPosition, maxPosition, pausePosition;

static short *screenLength, startLine, endLine;
static char *screenImage, **screenLine;

static int screenOptions;



/*****************************************************************************/
/*  Prototypes                                                               */
/*****************************************************************************/

void screenOpen(char *);
void screenControl(char *, int);
void screenClose(void);
void screenUpdate(WindowPtr);
void screenDisplay(char *);
void screenDump(char *, long);

char *macfgets(char *, int, FILE *);
int  macfprintf(FILE *, char *, ...);
int  macprintf(char *, ...);
int  macgetch(void);


/*****************************************************************************/
/*  Functions                                                                */
/*****************************************************************************/

void screenOpen(char *Title) {
    FontInfo fontInfo;
    int n;
    short       fontFamID;

    theWindow = GetNewWindow(screenWindow, nil, (WindowPtr)(-1));

    if ((Title != NULL) && (*Title != '\0')) {
        c2pstr(Title);
        SetWTitle(theWindow, (StringPtr)Title);
        p2cstr((StringPtr)Title);
    }

    ShowWindow(theWindow);

    SetPort(theWindow);
    GetFNum( "\pMonaco", &fontFamID );
    TextFont(fontFamID);
    TextSize(9);

    GetFontInfo(&fontInfo);
    fontHeight = fontInfo.ascent + fontInfo.descent + fontInfo.leading;
    fontWidth = fontInfo.widMax;

    scrollRgn = NewRgn();

    screenWidth = (theWindow->portRect.right - theWindow->portRect.left - 10) /
        fontWidth;
    screenHeight = (theWindow->portRect.bottom - theWindow->portRect.top) /
        fontHeight;
    maxPosition = screenHeight * fontHeight;
    pausePosition = maxPosition - (currentPosition = fontHeight);

    SetRect(&scrollRect, theWindow->portRect.left,
            theWindow->portRect.top + fontInfo.descent,
            theWindow->portRect.right,
            theWindow->portRect.bottom);
    SetRect(&pauseRect, theWindow->portRect.left,
            pausePosition + fontInfo.descent,
            theWindow->portRect.right,
            theWindow->portRect.bottom);

    MoveTo(5, currentPosition);

    n = (sizeof(char *) + sizeof(short) + screenWidth) * screenHeight;

    screenLine = (char **)NewPtr(n);

    screenLength = (short *)&screenLine[screenHeight];
    screenImage = (char *)&screenLength[screenHeight];

    for (n = 0; n < screenHeight; n++) {
        screenLine[n] = &screenImage[n * screenWidth];
        screenLength[n] = 0;
    }

    startLine = endLine = 0;

    screenOptions = 0;

    return;
}




void screenControl(char *options, int setting) {
    int n = 0;

    while (*options) {
        switch (*options) {
        case 'p':
            n |= pauseOption;
            break;
        case 's':
            n |= scrollOption;
            break;
        default:
            break;
        }
        options += 1;
    }

    if (setting == 0)
        screenOptions &= (n ^ (-1));
    else
        screenOptions |= n;

    if ((pausePosition = maxPosition - currentPosition) == 0)
        pausePosition = maxPosition - fontHeight;

    return;
}




void screenClose(void) {
    DisposePtr((Ptr)screenLine);

    DisposeWindow(theWindow);

    return;
}




void screenUpdate(WindowPtr window) {
    GrafPort *savePort;
    int m, n;

    if (window == theWindow) {
        BeginUpdate(window);
        if (!EmptyRgn(window->visRgn)) {
            GetPort(&savePort);
            SetPort(window);
            n = startLine;
            for (m = 1; ; m++) {
                MoveTo(5, m * fontHeight);
                if (screenLength[n] != 0)
                    DrawText(screenLine[n], 0, screenLength[n]);
                if (n == endLine) break;
                if ((n += 1) == screenHeight) n = 0;
            }
            SetPort(savePort);
        }
        EndUpdate(window);
    }

    return;
}




static void screenNewline(void) {
    MoveTo(5, currentPosition += fontHeight);
    if (currentPosition > maxPosition) {
        if (screenOptions & scrollOption) {
            ScrollRect(&scrollRect, 0, -fontHeight, scrollRgn);
            MoveTo(5, currentPosition = maxPosition);
            if ((startLine += 1) == screenHeight) startLine = 0;
        } else {
            ScrollRect(&scrollRect, 0, -maxPosition + fontHeight, scrollRgn);
            MoveTo(5, currentPosition = fontHeight + fontHeight);
            startLine = endLine;
        }
    }
    pausePosition -= fontHeight;

    if ((endLine += 1) == screenHeight) endLine = 0;
    screenLength[endLine] = 0;

    return;
}




static char waitChar(void) {
    WindowPtr whichWindow;
    EventRecord theEvent;

    for ( ; ; ) {
        SystemTask();
        if (GetNextEvent(everyEvent, &theEvent)) {
            switch (theEvent.what) {
            case keyDown:
                if ((theEvent.modifiers & cmdKey) &&
                    ((theEvent.message & charCodeMask) == '.'))
                    ExitToShell();
                return(theEvent.message & charCodeMask);
            case mouseDown:
                if (FindWindow(theEvent.where, &whichWindow) == inSysWindow)
                    SystemClick(&theEvent, whichWindow);
                break;
            case updateEvt:
                screenUpdate((WindowPtr)theEvent.message);
                break;
            }
        }
    }
}




static void screenPause(void) {
    if (pausePosition == 0) {
        if (screenOptions & pauseOption) {
            DrawText("Press any key to continue ...", 0, 29);
            memcpy(screenLine[endLine], "Press any key to continue ...", 29);
            screenLength[endLine] = 29;

            (void)waitChar();

            EraseRect(&pauseRect);
            MoveTo(5, currentPosition);
            screenLength[endLine] = 0;
        }

        pausePosition = maxPosition - fontHeight;
    }

    return;
}




void screenDisplay(char *s) {
    GrafPort *savePort;
    int m, n;
    char *t;

    GetPort(&savePort);
    SetPort(theWindow);

    while (*s) {
        screenPause();

        for (t = s; (*s) && (*s != '\n') && (*s != '\r'); s++)
            ;  /* empty body */

        if ((n = s - t) > (m = screenWidth - screenLength[endLine])) n = m;

        if (n > 0) {
            DrawText(t, 0, n);
            memcpy(screenLine[endLine] + screenLength[endLine], t, n);
            screenLength[endLine] += n;
        }

        if ((*s == '\n') || (*s == '\r')) {
            screenNewline();
            s += 1;
        }
    }

    SetPort(savePort);

    return;
}




void screenDump(char *s, long n) {
    GrafPort *savePort;
    int k, m;
    char *t;

    GetPort(&savePort);
    SetPort(theWindow);

    while (n) {
        screenPause();

        for (t = s; (n) && (*s != '\n') && (*s != '\r'); s++, n--)
            ;  /* empty body */

        if ((k = s - t) > (m = screenWidth - screenLength[endLine])) k = m;

        if (k > 0) {
            DrawText(t, 0, k);
            memcpy(screenLine[endLine] + screenLength[endLine], t, k);
            screenLength[endLine] += k;
        }

        if ((*s == '\n') || (*s == '\r')) {
            screenNewline();
            s += 1;
            n -= 1;
        }
    }

    SetPort(savePort);

    return;
}




char *macfgets(char *s, int n, FILE *stream) {
    GrafPort *savePort;
    char c, *t = s;

    stream = stream;

    GetPort(&savePort);
    SetPort(theWindow);

    for (n -= 1; (n > 0) && ((c = waitChar()) != '\r'); n -= 1) {
        DrawChar(*t++ = c);
        if (screenLength[endLine] < screenWidth)
            screenLine[endLine][screenLength[endLine]++] = c;
    }

    if (c == '\r') screenNewline();

    *t = '\0';

    SetPort(savePort);

    return(s);
}




int macfprintf(FILE *stream, char *format, ...)
{
    char buffer[bufferSize];
    va_list ap;
    int rc;

    stream = stream;

    va_start(ap, format);
    rc = vsprintf(buffer, format, ap);
    va_end(ap);

    screenDisplay(buffer);

    return rc;
}




int macprintf(char *format, ...)
{
    char buffer[bufferSize];
    va_list ap;
    int rc;

    va_start(ap, format);
    rc = vsprintf(buffer, format, ap);
    va_end(ap);

    screenDisplay(buffer);

    return rc;
}



/***********************/
/* Function macgetch() */
/***********************/

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
                screenUpdate((WindowPtr)theEvent.message);
                break;
            }
        }
    } while (theEvent.what != keyDown);

    macprintf("*");

    return (int)c;
}
