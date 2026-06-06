#include <stdio.h>
#ifdef rename
#undef rename
#endif
int (*foo)(const char *, const char *) = rename;
int main(void) { return 0; }
