#include <stdlib.h>
#ifdef atof
#undef atof
#endif
double (*foo)(const char *) = atof;
int main(void) { return 0; }
