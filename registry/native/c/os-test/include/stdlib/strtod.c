#include <stdlib.h>
#ifdef strtod
#undef strtod
#endif
double (*foo)(const char *restrict, char **restrict) = strtod;
int main(void) { return 0; }
