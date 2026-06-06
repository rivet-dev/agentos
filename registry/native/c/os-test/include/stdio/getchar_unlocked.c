#include <stdio.h>
#ifdef getchar_unlocked
#undef getchar_unlocked
#endif
int (*foo)(void) = getchar_unlocked;
int main(void) { return 0; }
