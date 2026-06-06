#include <stdio.h>
#ifdef getdelim
#undef getdelim
#endif
ssize_t (*foo)(char **restrict, size_t *restrict, int, FILE *restrict) = getdelim;
int main(void) { return 0; }
