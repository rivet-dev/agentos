/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef lcong48
#undef lcong48
#endif
void (*foo)(unsigned short [7]) = lcong48;
int main(void) { return 0; }
