#include <ctype.h>
#ifdef tolower
#undef tolower
#endif
int (*foo)(int) = tolower;
int main(void) { return 0; }
