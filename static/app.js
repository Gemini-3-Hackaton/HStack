document.addEventListener('DOMContentLoaded', () => {
    // Auth Elements
    const authOverlay = document.getElementById('auth-overlay');
    const authForm = document.getElementById('auth-form');
    const authTitle = document.getElementById('auth-title');
    const authSubmitBtn = document.getElementById('auth-submit-btn');
    const authToggleLink = document.getElementById('auth-toggle-link');
    const authToggleText = document.getElementById('auth-toggle-text');
    const authError = document.getElementById('auth-error');
    
    const registerFields = document.getElementById('register-fields');
    const firstNameInput = document.getElementById('auth-first-name');
    const lastNameInput = document.getElementById('auth-last-name');
    const passwordInput = document.getElementById('auth-password');

    // App Elements
    const taskList = document.getElementById('task-list');
    const chatForm = document.getElementById('chat-form');
    const chatInput = document.getElementById('chat-input');
    const chatSubmit = document.getElementById('chat-submit');
    const userHeader = document.querySelector('.user-header h1');

    let currentUser = JSON.parse(localStorage.getItem('hstack_user'));
    let isLoginMode = true;

    // --- Authentication Logic ---
    const checkAuthStatus = () => {
        if (currentUser) {
            authOverlay.classList.add('hidden');
            userHeader.textContent = `Hi ${currentUser.first_name},`;
            loadTasks();
        } else {
            authOverlay.classList.remove('hidden');
        }
    };

    authToggleLink.addEventListener('click', (e) => {
        e.preventDefault();
        isLoginMode = !isLoginMode;
        authError.textContent = '';
        
        if (isLoginMode) {
            authTitle.textContent = 'Log in to HStack';
            registerFields.style.display = 'none';
            authSubmitBtn.textContent = 'Login';
            authToggleText.textContent = "Don't have an account? ";
            authToggleLink.textContent = "Sign up";
        } else {
            authTitle.textContent = 'Create an account';
            registerFields.style.display = 'block';
            authSubmitBtn.textContent = 'Sign up';
            authToggleText.textContent = "Already have an account? ";
            authToggleLink.textContent = "Log in";
        }
    });

    authForm.addEventListener('submit', async (e) => {
        e.preventDefault();
        authError.textContent = '';
        authSubmitBtn.disabled = true;

        const url = isLoginMode ? '/api/auth/login' : '/api/auth/register';
        const payload = isLoginMode 
            ? { first_name: firstNameInput.value.trim(), password: passwordInput.value }
            : { first_name: firstNameInput.value.trim(), last_name: lastNameInput.value.trim(), password: passwordInput.value };

        try {
            const response = await fetch(url, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            });

            const data = await response.json();

            if (response.ok) {
                currentUser = data; // {id, first_name}
                localStorage.setItem('hstack_user', JSON.stringify(currentUser));
                checkAuthStatus();
            } else {
                authError.textContent = data.detail || 'Authentication failed.';
            }
        } catch (error) {
            authError.textContent = 'Network error connecting to server.';
        } finally {
            authSubmitBtn.disabled = false;
        }
    });

    const formatDate = (dateString) => {
        const date = new Date(dateString);
        return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    };

    const createTicketCard = (task) => {
        const payload = task.payload || {};
        const isCompleted = payload.completed === true;
        const title = payload.title || 'Untitled';
        const type = task.type || 'TASK';

        const card = document.createElement('div');
        card.className = `ticket-card ${isCompleted ? 'completed' : ''}`;
        card.dataset.id = task.id;

        card.innerHTML = `
            <div class="ticket-status"></div>
            <div class="ticket-content">
                <div class="ticket-title">${title}</div>
                <div class="ticket-meta">
                    <span class="type-badge ${type.toLowerCase()}">${type}</span>
                    <span class="ticket-date">${formatDate(task.created_at)}</span>
                    <span style="display:none;" class="ticket-id-hidden">${task.id}</span>
                </div>
            </div>
        `;

        const statusIcon = card.querySelector('.ticket-status');
        statusIcon.addEventListener('click', async (e) => {
            e.stopPropagation();
            const completing = !card.classList.contains('completed');
            if (completing) {
                card.classList.add('completed');
                task.payload.completed = true;
            } else {
                card.classList.remove('completed');
                task.payload.completed = false;
            }
        });

        return card;
    };

    const loadTasks = async () => {
        if (!currentUser) return;

        try {
            taskList.innerHTML = '<div style="color:#8A8F98; text-align:center; padding: 20px;">Fetching from your Stack...</div>';
            const response = await fetch(`/api/tasks?userid=${currentUser.id}`);
            if (!response.ok) throw new Error("Failed to fetch tasks");
            
            const tasks = await response.json();
            
            taskList.innerHTML = '';
            if (tasks.length === 0) {
                 taskList.innerHTML = '<div style="color:#8A8F98; text-align:center; padding: 20px;">Your stack is empty! Ask Gemini below to add a Habit, Event, or Task.</div>';
            } else {
                 tasks.forEach(task => {
                    taskList.appendChild(createTicketCard(task));
                 });
            }
        } catch (error) {
            console.error(error);
            taskList.innerHTML = `<div style="color:#D93025; text-align:center; padding: 20px;">Database error: ${error.message} - Check your DB connection.</div>`;
        }
    };

    // Auto-resize chat textarea
    chatInput.addEventListener('input', function() {
        this.style.height = 'auto';
        this.style.height = (this.scrollHeight) + 'px';
        if(this.value === '') {
            this.style.height = '48px'; 
        }
    });

    // Enter to submit, Shift+Enter for new line in textarea
    chatInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            if(chatInput.value.trim()) {
                chatForm.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
            }
        }
    });

    // --- WebGL Shader Logic ---
    const canvas = document.getElementById("glCanvas");
    const inputWrapper = document.querySelector('.chat-input-wrapper');
    const cssFallback = document.getElementById("cssFallback");
    let gl, program, animationFrameId, startTime = 0, isProcessing = false;

    const initWebGL = () => {
        try {
            gl = canvas.getContext("webgl") || canvas.getContext("experimental-webgl");
            if (!gl) throw new Error("WebGL Not Supported");

            const vsSource = document.getElementById("vertexShader").textContent;
            const fsSource = document.getElementById("fragmentShader").textContent;

            const createShader = (gl, type, source) => {
                const shader = gl.createShader(type);
                gl.shaderSource(shader, source);
                gl.compileShader(shader);
                return shader;
            };

            const vs = createShader(gl, gl.VERTEX_SHADER, vsSource);
            const fs = createShader(gl, gl.FRAGMENT_SHADER, fsSource);

            program = gl.createProgram();
            gl.attachShader(program, vs);
            gl.attachShader(program, fs);
            gl.linkProgram(program);

            const positionAttributeLocation = gl.getAttribLocation(program, "position");
            const positionBuffer = gl.createBuffer();
            gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
            gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 1,-1, -1,1, -1,1, 1,-1, 1,1]), gl.STATIC_DRAW);

            gl.useProgram(program);
            gl.enableVertexAttribArray(positionAttributeLocation);
            gl.vertexAttribPointer(positionAttributeLocation, 2, gl.FLOAT, false, 0, 0);

            cssFallback.style.display = 'none';
            return true;
        } catch (e) {
            console.warn("WebGL Init Failed, using CSS fallback", e);
            cssFallback.style.display = 'block';
            return false;
        }
    };

    const resizeCanvas = () => {
        if (!gl) return;
        const displayWidth = canvas.clientWidth;
        const displayHeight = canvas.clientHeight;
        if (canvas.width !== displayWidth || canvas.height !== displayHeight) {
            canvas.width = displayWidth;
            canvas.height = displayHeight;
            gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
        }
    };

    const renderShader = (timestamp) => {
        if (!isProcessing || !gl) return;
        if (startTime === 0) startTime = timestamp;
        const time = (timestamp - startTime) * 0.001;

        gl.uniform1f(gl.getUniformLocation(program, "u_time"), time);
        gl.uniform2f(gl.getUniformLocation(program, "u_resolution"), canvas.width, canvas.height);
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        animationFrameId = requestAnimationFrame(renderShader);
    };

    const webglAvailable = initWebGL();
    window.addEventListener('resize', resizeCanvas);

    // Handle Gemini Action Model Submission
    chatForm.addEventListener('submit', async (e) => {
        e.preventDefault();
        
        const message = chatInput.value.trim();
        if (!message || !currentUser) return;

        chatInput.value = '';
        chatInput.placeholder = 'Gemini is processing your action...';
        chatInput.style.height = '48px';
        chatSubmit.disabled = true;
        
        // Start Animation
        inputWrapper.classList.add('processing');
        isProcessing = true;
        if (webglAvailable) {
            resizeCanvas();
            startTime = 0;
            renderShader(performance.now());
        }

        try {
            const response = await fetch('/api/chat', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ message: message, userid: currentUser.id })
            });

            const data = await response.json();

            if (response.ok) {
                await loadTasks();
                
                // Always show the AI's confirmation text if it exists
                if (data.response) {
                    showAIFeedback(data.response);
                } else if (data.action && data.action !== 'message') {
                    chatInput.placeholder = 'Action completed. Tell Gemini to manage tickets...';
                }
            } else {
                chatInput.placeholder = 'Error communicating with AI.';
            }
        } catch (error) {
            console.error(error);
            chatInput.placeholder = 'Network error reaching the server.';
        } finally {
            // Stop Animation
            isProcessing = false;
            cancelAnimationFrame(animationFrameId);
            inputWrapper.classList.remove('processing');

            chatSubmit.disabled = false;
            setTimeout(() => {
                if(chatInput.placeholder.includes('Action completed')) {
                   chatInput.placeholder = 'Tell Gemini to manage tickets...';
                }
            }, 3000);
            chatInput.focus();
        }
    });

    const showAIFeedback = (msg) => {
        const feedbackArea = document.getElementById('ai-feedback');
        feedbackArea.textContent = msg;
        feedbackArea.classList.add('visible');
        
        setTimeout(() => {
            feedbackArea.classList.remove('visible');
        }, 5000);
    };

    // Boot app
    checkAuthStatus();
});
