# kampus
long term memory for coding repos
Using awk
awk is another powerful text processing tool that allows you to specify conditions based on line numbers (represented by the built-in variable NR). 
Command: awk 'NR >= M && NR <= N' filename.txt
Example: To read lines 10 to 20 of data.txt, you would use:
bash
awk 'NR >= 10 && NR <= 20' data.txt
NR stands for "Number of Record" (line number).
The condition NR >= 10 && NR <= 20 is evaluated for each line, and if true, the default action (printing the line) is performed. 
Using head and tail
You can combine head and tail commands to achieve the desired result by piping the output of one command to the other. 
Command: head -n N filename
