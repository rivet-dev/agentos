#include <ctype.h>
#ifdef toupper
#undef toupper
#endif
int (*foo)(int) = toupper;
int main(void) { return 0; }
