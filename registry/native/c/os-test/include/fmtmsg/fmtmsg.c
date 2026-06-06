/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <fmtmsg.h>
#ifdef fmtmsg
#undef fmtmsg
#endif
int (*foo)(long, const char *, int, const char *, const char *, const char *) = fmtmsg;
int main(void) { return 0; }
