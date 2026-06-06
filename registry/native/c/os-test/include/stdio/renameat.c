#include <stdio.h>
#ifdef renameat
#undef renameat
#endif
int (*foo)(int, const char *, int, const char *) = renameat;
int main(void) { return 0; }
