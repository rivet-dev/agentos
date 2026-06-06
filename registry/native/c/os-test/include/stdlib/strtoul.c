#include <stdlib.h>
#ifdef strtoul
#undef strtoul
#endif
unsigned long (*foo)(const char *restrict, char **restrict, int) = strtoul;
int main(void) { return 0; }
