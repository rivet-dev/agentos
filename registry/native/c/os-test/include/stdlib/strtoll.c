#include <stdlib.h>
#ifdef strtoll
#undef strtoll
#endif
long long (*foo)(const char *restrict, char **restrict, int) = strtoll;
int main(void) { return 0; }
