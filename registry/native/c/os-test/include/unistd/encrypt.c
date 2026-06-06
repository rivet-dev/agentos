/*[OB XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef encrypt
#undef encrypt
#endif
void (*foo)(char [64], int) = encrypt;
int main(void) { return 0; }
