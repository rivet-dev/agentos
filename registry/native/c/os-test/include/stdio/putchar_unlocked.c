#include <stdio.h>
#ifdef putchar_unlocked
#undef putchar_unlocked
#endif
int (*foo)(int) = putchar_unlocked;
int main(void) { return 0; }
