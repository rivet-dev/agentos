#include <stdio.h>
#ifdef getchar
#undef getchar
#endif
int (*foo)(void) = getchar;
int main(void) { return 0; }
