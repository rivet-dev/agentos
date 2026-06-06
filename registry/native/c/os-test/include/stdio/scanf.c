#include <stdio.h>
#ifdef scanf
#undef scanf
#endif
int (*foo)(const char *restrict, ...) = scanf;
int main(void) { return 0; }
