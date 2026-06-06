#include <stdlib.h>
#ifdef strtol
#undef strtol
#endif
long (*foo)(const char *restrict, char **restrict, int) = strtol;
int main(void) { return 0; }
