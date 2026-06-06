#include <stdio.h>
#ifdef putc_unlocked
#undef putc_unlocked
#endif
int (*foo)(int, FILE *) = putc_unlocked;
int main(void) { return 0; }
