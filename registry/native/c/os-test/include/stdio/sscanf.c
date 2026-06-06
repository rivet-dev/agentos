#include <stdio.h>
#ifdef sscanf
#undef sscanf
#endif
int (*foo)(const char *restrict, const char *restrict, ...) = sscanf;
int main(void) { return 0; }
