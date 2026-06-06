#include <stdio.h>
#ifdef getc_unlocked
#undef getc_unlocked
#endif
int (*foo)(FILE *) = getc_unlocked;
int main(void) { return 0; }
