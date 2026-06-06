#include <regex.h>
#ifdef regfree
#undef regfree
#endif
void (*foo)(regex_t *) = regfree;
int main(void) { return 0; }
