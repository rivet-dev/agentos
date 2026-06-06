/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ndbm.h>
#ifdef dbm_nextkey
#undef dbm_nextkey
#endif
datum (*foo)(DBM *) = dbm_nextkey;
int main(void) { return 0; }
