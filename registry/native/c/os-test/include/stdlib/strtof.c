#include <stdlib.h>
#ifdef strtof
#undef strtof
#endif
float (*foo)(const char *restrict, char **restrict) = strtof;
int main(void) { return 0; }
