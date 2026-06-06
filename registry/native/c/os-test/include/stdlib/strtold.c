#include <stdlib.h>
#ifdef strtold
#undef strtold
#endif
long double (*foo)(const char *restrict, char **restrict) = strtold;
int main(void) { return 0; }
